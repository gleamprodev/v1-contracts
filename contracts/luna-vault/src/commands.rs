use std::borrow::BorrowMut;

use cosmwasm_std::{Api, attr, BankMsg, Coin, coins, CosmosMsg, Decimal, DepsMut, Env, from_binary, MessageInfo, Response, StdError, StdResult, Storage, to_binary, Uint128, WasmMsg};
use cw20::{Cw20ExecuteMsg, Cw20ReceiveMsg};
use terraswap::asset::{Asset, AssetInfo};
use terraswap::querier::query_supply;

use signed_integer::SignedInt;
use white_whale::anchor::anchor_deposit_msg;
use white_whale::astroport_helper::{create_astroport_lp_msg, create_astroport_msg};
use white_whale::fee::Fee;
use white_whale::luna_vault::msg::Cw20HookMsg;
use white_whale::memory::LIST_SIZE_LIMIT;

use crate::contract::VaultResult;
use crate::error::LunaVaultError;
use crate::helpers::{check_fee, compute_total_value, get_treasury_fee, slashing};
use crate::math::decimal_division;
use crate::pool_info::PoolInfoRaw;
use crate::state::{ADMIN, CURRENT_BATCH, DEPOSIT_INFO, FEE, get_finished_amount, get_unbond_batches, PARAMETERS, POOL_INFO, PROFIT, read_unbond_history, remove_unbond_wait_list, STATE, State, store_unbond_history, store_unbond_wait_list, UnbondHistory};

/// handler function invoked when the luna-vault contract receives
/// a transaction. In this case it is triggered when the LP tokens are deposited
/// into the contract
pub fn receive_cw20(
    deps: DepsMut,
    env: Env,
    msg_info: MessageInfo,
    cw20_msg: Cw20ReceiveMsg,
) -> VaultResult {
    match from_binary(&cw20_msg.msg)? {
        Cw20HookMsg::Unbond {} => {
            // only vLuna token contract can execute this message
            let info: PoolInfoRaw = POOL_INFO.load(deps.storage)?;
            if deps.api.addr_validate(&msg_info.sender.to_string())? != info.liquidity_token {
                return Err(LunaVaultError::Unauthorized {});
            }
            unbond(deps, env, cw20_msg.amount, cw20_msg.sender)
        }
    }
}


// Deposits Luna into the contract.
pub fn provide_liquidity(
    mut deps: DepsMut,
    env: Env,
    msg_info: MessageInfo,
    asset: Asset,
) -> VaultResult {
    let deposit_info = DEPOSIT_INFO.load(deps.storage)?;
    let profit = PROFIT.load(deps.storage)?;
    let info: PoolInfoRaw = POOL_INFO.load(deps.storage)?;

    if profit.last_balance != Uint128::zero() {
        return Err(LunaVaultError::DepositDuringLoan {});
    }

    // Init vector for logging
    let mut attrs = vec![];
    // Check if deposit matches claimed deposit.
    deposit_info.assert(&asset.info)?;
    asset.assert_sent_native_token_balance(&msg_info)?;
    attrs.push(("Action:", String::from("Deposit to vault")));
    attrs.push(("Received funds:", asset.to_string()));

    let params = PARAMETERS.load(deps.storage)?;
    let threshold = params.er_threshold;
    let recovery_fee = params.peg_recovery_fee;

    // current batch requested fee is needed for accurate exchange rate computation.
    let current_batch = CURRENT_BATCH.load(deps.storage)?;
    let requested_with_fee = current_batch.requested_with_fee;

    // Received deposit to vault
    let deposit: Uint128 = asset.amount;

    // check slashing
    let mut state = STATE.load(deps.storage)?;
    slashing(&mut deps, env.clone(), &mut state, &params)?;

    // get the total vluna supply
    let mut total_supply = query_supply(&deps.querier, info.liquidity_token.clone())?;

    // peg recovery fee should be considered
    let mint_amount = decimal_division(deposit, state.exchange_rate);
    let mut mint_amount_with_fee = mint_amount;
    if state.exchange_rate < threshold {
        let max_peg_fee = mint_amount * recovery_fee;
        let required_peg_fee = ((total_supply + mint_amount + current_batch.requested_with_fee)
            .checked_sub(state.total_bond_amount + deposit))?;
        let peg_fee = Uint128::min(max_peg_fee, required_peg_fee);
        mint_amount_with_fee = (mint_amount.checked_sub(peg_fee))?;
    }

    // total supply should be updated for exchange rate calculation.
    total_supply += mint_amount_with_fee;

    // exchange rate should be updated for future
    state.total_bond_amount += deposit;
    state.update_exchange_rate(total_supply, requested_with_fee);
    STATE.save(deps.storage, &state)?;

    // mint LP token to sender
    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: info.liquidity_token.to_string(),
        msg: to_binary(&Cw20ExecuteMsg::Mint {
            recipient: msg_info.sender.to_string(),
            amount: mint_amount_with_fee,
        })?,
        funds: vec![],
    });

    let response = Response::new().add_attributes(attrs).add_message(msg);
    // If contract holds more than ASTROPORT_DEPOSIT_THRESHOLD [LUNA] then try deposit to Astroport and leave LUNA_CAP [LUNA] in contract.
    let (_, luna_in_contract, _, _, _) = compute_total_value(&env, deps.as_ref(), &info)?;
    return if luna_in_contract > info.luna_cap {
        _deposit_passive_strategy(response)
    } else {
        Ok(response)
    };
}

// Deposits Luna into the passive strategy (Astroport) -> luna-bluna LP
fn _deposit_passive_strategy(response: Response) -> VaultResult {
    //let deposit_msg = create_astroport_lp_msg();
    //Ok(response.add_message(deposit_msg))
    Ok(response)
}

/// This message must be called by receive_cw20
/// This message will trigger the withdrawal waiting time and burn vluna token
fn unbond(
    mut deps: DepsMut,
    env: Env,
    amount: Uint128,
    sender: String, // human who sent the vluna to us
) -> VaultResult {
    let profit = PROFIT.load(deps.storage)?;
    if profit.last_balance != Uint128::zero() {
        return Err(LunaVaultError::DepositDuringLoan {});
    }

    // Logging var
    let mut attrs = vec![];
    attrs.push(("from", sender.clone()));
    attrs.push(("burnt_amount", amount.to_string()));

    let mut current_batch = CURRENT_BATCH.load(deps.storage)?;

    // Check slashing, update state, and calculate the new exchange rate.
    let params = PARAMETERS.load(deps.storage)?;
    let mut state = STATE.load(deps.storage)?;
    slashing(&mut deps, env.clone(), &mut state, &params)?;

    let info: PoolInfoRaw = POOL_INFO.load(deps.storage)?;
    let mut total_supply = query_supply(&deps.querier, info.liquidity_token.clone())?;

    // Get treasury fee in LP tokens
    let treasury_fee = get_treasury_fee(deps.as_ref(), amount)?;

    // Collect all the requests within a epoch period
    // Apply peg recovery fee
    let mut amount_with_fee = if state.exchange_rate < params.er_threshold {
        let max_peg_fee = amount * params.peg_recovery_fee;
        let required_peg_fee = ((total_supply + current_batch.requested_with_fee)
            .checked_sub(state.total_bond_amount))?;
        let peg_fee = Uint128::min(max_peg_fee, required_peg_fee);
        (amount.checked_sub(peg_fee))?
    } else {
        amount
    };
    // substract the treasury fee
    amount_with_fee = amount_with_fee.checked_sub(treasury_fee)?;
    attrs.push(("Post-fee unbonded_amount:", amount_with_fee.to_string()));

    current_batch.requested_with_fee += amount_with_fee;

    let sender_addr = deps.api.addr_validate(&sender)?;
    store_unbond_wait_list(
        deps.storage,
        current_batch.id,
        &sender_addr,
        amount_with_fee,
    )?;

    total_supply = (total_supply.checked_sub(amount))
        .expect("the requested can not be more than the total supply");

    // Update exchange rate
    state.update_exchange_rate(total_supply, current_batch.requested_with_fee);

    let passed_time_seconds = env
        .block
        .time
        .minus_seconds(state.last_unbonded_time)
        .seconds();

    // If the epoch period is passed, the undelegate message would be sent.
    if passed_time_seconds > params.epoch_period {
        // Apply the current exchange rate.
        let undelegation_amount = current_batch.requested_with_fee * state.exchange_rate;

        // the contract must stop if
        if undelegation_amount == Uint128::new(1) {
            return Err(LunaVaultError::TooSmallBurn {});
        }

        state.total_bond_amount = (state.total_bond_amount.checked_sub(undelegation_amount))
            .expect("Undelegation amount can not be more than stored total bonded amount");

        // Store history for withdraw unbonded
        let history = UnbondHistory {
            batch_id: current_batch.id,
            time: env.block.time.seconds(),
            amount: current_batch.requested_with_fee,
            applied_exchange_rate: state.exchange_rate,
            withdraw_rate: state.exchange_rate,
            released: false,
        };
        store_unbond_history(deps.storage, current_batch.id, history)?;

        // batch info must be updated to new batch
        current_batch.id += 1;
        current_batch.requested_with_fee = Uint128::zero();

        // state.last_unbonded_time must be updated to the current block time
        state.last_unbonded_time = env.block.time.seconds();
    }

    // Store the new requested_with_fee or id in the current batch
    CURRENT_BATCH.save(deps.storage, &current_batch)?;

    // Store state's new exchange rate
    STATE.save(deps.storage, &state)?;

    // LP token treasury Asset
    let lp_token_treasury_fee = Asset {
        info: AssetInfo::Token {
            contract_addr: info.liquidity_token.to_string(),
        },
        amount: treasury_fee,
    };

    // Construct treasury fee msg.
    let fee_config = FEE.load(deps.storage)?;
    let treasury_fee_msg = fee_config.treasury_fee.msg(
        deps.as_ref(),
        lp_token_treasury_fee,
        fee_config.treasury_addr,
    )?;
    attrs.push(("Treasury fee:", treasury_fee.to_string()));

    // Send Burn message to vluna contract
    let burn_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: info.liquidity_token.to_string(),
        // Burn excludes treasury fee
        msg: to_binary(&Cw20ExecuteMsg::Burn { amount: amount - treasury_fee })?,
        funds: vec![],
    });

    Ok(Response::new()
        .add_message(burn_msg)
        .add_message(treasury_fee_msg)
        .add_attribute("action:", "unbound")
        .add_attributes(attrs))
}

pub fn execute_withdraw_unbonded(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
) -> VaultResult {
    // read params
    let params = PARAMETERS.load(deps.storage)?;
    let coin_denom = params.underlying_coin_denom;

    let historical_time = env.block.time.seconds() - params.unbonding_period;

    // query vault balance for process withdraw rate.
    let vault_balance = deps
        .querier
        .query_balance(&env.contract.address, &*coin_denom)?
        .amount;

    // calculate withdraw rate for user requests
    _process_withdraw_rate(deps.storage, historical_time, vault_balance)?;

    let withdraw_amount = get_finished_amount(deps.storage, &info.sender, None)?;
    if withdraw_amount.is_zero() {
        return Err(LunaVaultError::NoWithdrawableAssetsAvailable(coin_denom));
    }

    // remove the previous batches for the user
    let deprecated_batches = get_unbond_batches(deps.storage, &info.sender, None)?;
    remove_unbond_wait_list(deps.storage, deprecated_batches, &info.sender)?;

    // Update previous balance used for calculation in next Luna batch release
    let prev_balance = (vault_balance.checked_sub(withdraw_amount))?;
    STATE.update(deps.storage, |mut last_state| -> StdResult<State> {
        last_state.prev_vault_balance = prev_balance;
        Ok(last_state)
    })?;

    // Send the money to the user
    let withdraw_msg = CosmosMsg::Bank(BankMsg::Send {
        to_address: info.sender.to_string(),
        amount: coins(withdraw_amount.u128(), &*coin_denom),
    });

    Ok(Response::new()
        .add_attributes(vec![
            attr("action", "execute_withdraw_unbonded"),
            attr("from", env.contract.address),
            attr("amount", withdraw_amount),
        ])
        .add_message(withdraw_msg))
}

/// This is designed for an accurate unbonded amount calculation.
/// Execute while processing withdraw_unbonded
fn _process_withdraw_rate(
    storage: &mut dyn Storage,
    historical_time: u64,
    vault_balance: Uint128,
) -> StdResult<()> {
    // balance change of the vault contract must be checked.
    let mut total_unbonded_amount = Uint128::zero();

    let mut state = STATE.load(storage)?;

    let balance_change = SignedInt::from_subtraction(vault_balance, state.prev_vault_balance);
    state.actual_unbonded_amount += balance_change.0;

    let last_processed_batch = state.last_processed_batch;
    let mut batch_count: u64 = 0;

    // Iterate over unbonded histories that have been processed
    // to calculate newly added unbonded amount
    let mut i = last_processed_batch + 1;
    loop {
        let history: UnbondHistory;
        match read_unbond_history(storage, i) {
            Ok(h) => {
                if h.time > historical_time {
                    break;
                }
                if !h.released {
                    history = h.clone();
                } else {
                    break;
                }
            }
            Err(_) => break,
        }
        let burnt_amount = history.amount;
        let historical_rate = history.withdraw_rate;
        let unbonded_amount = burnt_amount * historical_rate;
        total_unbonded_amount += unbonded_amount;
        batch_count += 1;
        i += 1;
    }

    if batch_count >= 1 {
        // Use signed integer in case of some rogue transfers.
        let slashed_amount =
            SignedInt::from_subtraction(total_unbonded_amount, state.actual_unbonded_amount);

        // Iterate again to calculate the withdraw rate for each unprocessed history
        let mut iterator = last_processed_batch + 1;
        loop {
            let history: UnbondHistory;
            match read_unbond_history(storage, iterator) {
                Ok(h) => {
                    if h.time > historical_time {
                        break;
                    }
                    if !h.released {
                        history = h
                    } else {
                        break;
                    }
                }
                Err(_) => {
                    break;
                }
            }
            let burnt_amount_of_batch = history.amount;
            let historical_rate_of_batch = history.withdraw_rate;
            let unbonded_amount_of_batch = burnt_amount_of_batch * historical_rate_of_batch;

            // the slashed amount for each batch must be proportional to the unbonded amount of batch
            let batch_slashing_weight =
                Decimal::from_ratio(unbonded_amount_of_batch, total_unbonded_amount);

            let mut slashed_amount_of_batch = batch_slashing_weight * slashed_amount.0;
            let actual_unbonded_amount_of_batch: Uint128;

            // If slashed amount is negative, there should be summation instead of subtraction.
            if slashed_amount.1 {
                slashed_amount_of_batch = (slashed_amount_of_batch.checked_sub(Uint128::new(1)))?;
                actual_unbonded_amount_of_batch =
                    unbonded_amount_of_batch + slashed_amount_of_batch;
            } else {
                if slashed_amount.0.u128() != 0u128 {
                    slashed_amount_of_batch += Uint128::new(1);
                }
                actual_unbonded_amount_of_batch =
                    SignedInt::from_subtraction(unbonded_amount_of_batch, slashed_amount_of_batch)
                        .0;
            }
            // Calculate the new withdraw rate
            let new_withdraw_rate =
                Decimal::from_ratio(actual_unbonded_amount_of_batch, burnt_amount_of_batch);

            let mut history_for_i = history;
            // store the history and mark it as released
            history_for_i.withdraw_rate = new_withdraw_rate;
            history_for_i.released = true;
            store_unbond_history(storage, iterator, history_for_i)?;
            state.last_processed_batch = iterator;
            iterator += 1;
        }
    }
    // Store state.actual_unbonded_amount for future new batches release
    state.actual_unbonded_amount = Uint128::zero();
    STATE.save(storage, &state)?;

    Ok(())
}

/// Sets the liquid luna cap on the vault.
pub fn set_luna_cap(deps: DepsMut, msg_info: MessageInfo, luna_cap: Uint128) -> VaultResult {
    // Only the admin should be able to call this
    ADMIN.assert_admin(deps.as_ref(), &msg_info.sender)?;

    let mut info: PoolInfoRaw = POOL_INFO.load(deps.storage)?;
    let previous_cap = info.luna_cap;
    info.luna_cap = luna_cap;
    POOL_INFO.save(deps.storage, &info)?;
    Ok(Response::new()
        .add_attribute("new luna cap", luna_cap.to_string())
        .add_attribute("previous luna cap", previous_cap.to_string()))
}

/// Sets a new admin
pub fn set_admin(deps: DepsMut, info: MessageInfo, admin: String) -> VaultResult {
    let admin_addr = deps.api.addr_validate(&admin)?;
    let previous_admin = ADMIN.get(deps.as_ref())?.unwrap();
    ADMIN.execute_update_admin(deps, info, Some(admin_addr))?;
    Ok(Response::default()
        .add_attribute("previous admin", previous_admin)
        .add_attribute("admin", admin))
}

/// Sets new fees for vault, flashloan and treasury
pub fn set_fee(
    deps: DepsMut,
    msg_info: MessageInfo,
    flash_loan_fee: Option<Fee>,
    treasury_fee: Option<Fee>,
    commission_fee: Option<Fee>,
) -> VaultResult {
    // Only the admin should be able to call this
    ADMIN.assert_admin(deps.as_ref(), &msg_info.sender)?;
    let mut fee_config = FEE.load(deps.storage)?;

    if let Some(fee) = flash_loan_fee {
        fee_config.flash_loan_fee = check_fee(fee)?;
    }
    if let Some(fee) = treasury_fee {
        fee_config.treasury_fee = check_fee(fee)?;
    }
    if let Some(fee) = commission_fee {
        fee_config.commission_fee = check_fee(fee)?;
    }

    FEE.save(deps.storage, &fee_config)?;
    Ok(Response::default())
}

/// Adds a contract to the whitelist
pub fn add_to_whitelist(
    deps: DepsMut,
    msg_info: MessageInfo,
    contract_addr: String,
) -> VaultResult {
    // Only the admin should be able to call this
    ADMIN.assert_admin(deps.as_ref(), &msg_info.sender)?;

    let mut state = STATE.load(deps.storage)?;
    // Check if contract is already in whitelist
    if state
        .whitelisted_contracts
        .contains(&deps.api.addr_validate(&contract_addr)?)
    {
        return Err(LunaVaultError::AlreadyWhitelisted {});
    }

    // This is a limit to prevent potentially running out of gas when doing lookups on the whitelist
    if state.whitelisted_contracts.len() >= LIST_SIZE_LIMIT {
        return Err(LunaVaultError::WhitelistLimitReached {});
    }

    // Add contract to whitelist.
    state
        .whitelisted_contracts
        .push(deps.api.addr_validate(&contract_addr)?);
    STATE.save(deps.storage, &state)?;

    // Respond and note the change
    Ok(Response::new().add_attribute("Added contract to whitelist: ", contract_addr))
}

/// Removes a contract from the whitelist
pub fn remove_from_whitelist(
    deps: DepsMut,
    msg_info: MessageInfo,
    contract_addr: String,
) -> VaultResult {
    // Only the admin should be able to call this
    ADMIN.assert_admin(deps.as_ref(), &msg_info.sender)?;

    let mut state = STATE.load(deps.storage)?;
    // Check if contract is in whitelist
    if !state
        .whitelisted_contracts
        .contains(&deps.api.addr_validate(&contract_addr)?)
    {
        return Err(LunaVaultError::NotWhitelisted {});
    }

    // Remove contract from whitelist.
    let contract_validated_addr = deps.api.addr_validate(&contract_addr)?;
    state
        .whitelisted_contracts
        .retain(|addr| *addr != contract_validated_addr);
    STATE.save(deps.storage, &state)?;

    // Respond and note the change
    Ok(Response::new().add_attribute("Removed contract from whitelist: ", contract_addr))
}

/// Updates the contract state
pub fn update_state(
    deps: DepsMut,
    info: MessageInfo,
    bluna_address: Option<String>,
    memory_address: Option<String>,
    allow_non_whitelisted: Option<bool>,
) -> VaultResult {
    // Only the admin should be able to call this
    ADMIN.assert_admin(deps.as_ref(), &info.sender)?;

    let mut state = STATE.load(deps.storage)?;
    let api = deps.api;

    if let Some(bluna_address) = bluna_address {
        state.bluna_address = api.addr_validate(&bluna_address)?;
    }
    if let Some(memory_address) = memory_address {
        state.memory_address = api.addr_validate(&memory_address)?;
    }

    if let Some(allow_non_whitelisted) = allow_non_whitelisted {
        state.allow_non_whitelisted = allow_non_whitelisted;
    }

    STATE.save(deps.storage, &state)?;
    Ok(Response::new().add_attribute("Update:", "Successful"))
}