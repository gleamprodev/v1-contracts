use cosmwasm_std::{
    entry_point, to_binary, BankMsg, Binary, Coin, CosmosMsg, Deps, DepsMut, Env, MessageInfo,
    Response, StdResult, WasmMsg,
};

use terra_cosmwasm::{create_swap_msg, TerraMsgWrapper};
use terraswap::asset::{Asset, AssetInfo};

use terraswap::querier::query_balance;

use white_whale::denom::LUNA_DENOM;

use white_whale::astroport_helper::create_astroport_msg;

use white_whale::deposit_info::ArbBaseAsset;
use white_whale::query::astroport::simulate_swap as simulate_astroport_swap;
use white_whale::tax::deduct_tax;
use white_whale::ust_vault::msg::ExecuteMsg as VaultMsg;
use white_whale::ust_vault::msg::FlashLoanPayload;

use crate::error::StableArbError;
use crate::msg::{ArbDetails, CallbackMsg, ExecuteMsg, InstantiateMsg, QueryMsg};

use crate::querier::query_market_price;

use crate::state::{State, ADMIN, ARB_BASE_ASSET, STATE};

type VaultResult = Result<Response<TerraMsgWrapper>, StableArbError>;

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> VaultResult {
    let state = State {
        vault_address: deps.api.addr_canonicalize(&msg.vault_address)?,
        seignorage_address: deps.api.addr_canonicalize(&msg.seignorage_address)?,
        pool_address: deps.api.addr_canonicalize(&msg.pool_address)?,
    };

    // Store the initial config
    STATE.save(deps.storage, &state)?;
    ARB_BASE_ASSET.save(
        deps.storage,
        &ArbBaseAsset {
            asset_info: msg.asset_info,
        },
    )?;
    // Setup the admin as the creator of the contract
    ADMIN.set(deps, Some(info.sender))?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> VaultResult {
    match msg {
        ExecuteMsg::ExecuteArb { details, above_peg } => {
            call_flashloan(deps, env, info, details, above_peg)
        }
        ExecuteMsg::BelowPegCallback { details } => try_arb_below_peg(deps, env, info, details),
        ExecuteMsg::AbovePegCallback { details } => try_arb_above_peg(deps, env, info, details),
        ExecuteMsg::SetAdmin { admin } => {
            let admin_addr = deps.api.addr_validate(&admin)?;
            let previous_admin = ADMIN.get(deps.as_ref())?.unwrap();
            ADMIN.execute_update_admin(deps, info, Some(admin_addr))?;
            Ok(Response::default()
                .add_attribute("previous admin", previous_admin)
                .add_attribute("admin", admin))
        }
        ExecuteMsg::Callback(msg) => _handle_callback(deps, env, info, msg),
    }
}

//----------------------------------------------------------------------------------------
//  PRIVATE FUNCTIONS
//----------------------------------------------------------------------------------------

fn _handle_callback(deps: DepsMut, env: Env, info: MessageInfo, msg: CallbackMsg) -> VaultResult {
    // Callback functions can only be called this contract itself
    if info.sender != env.contract.address {
        return Err(StableArbError::NotCallback {});
    }
    match msg {
        CallbackMsg::AfterSuccessfulTradeCallback {} => after_successful_trade_callback(deps, env),
        // Possibility to add more callbacks in future.
    }
}
//----------------------------------------------------------------------------------------
//  EXECUTE FUNCTION HANDLERS
//----------------------------------------------------------------------------------------

fn call_flashloan(
    deps: DepsMut,
    _env: Env,
    _msg_info: MessageInfo,
    details: ArbDetails,
    above_peg: bool,
) -> VaultResult {
    let state = STATE.load(deps.storage)?;
    let deposit_info = ARB_BASE_ASSET.load(deps.storage)?;

    // Check if requested asset is same as strategy base asset
    deposit_info.assert(&details.asset.info)?;

    // Construct callback msg
    let callback_msg;
    if above_peg {
        callback_msg = ExecuteMsg::AbovePegCallback {
            details: details.clone(),
        }
    } else {
        callback_msg = ExecuteMsg::BelowPegCallback {
            details: details.clone(),
        }
    }

    // Construct payload
    let payload = FlashLoanPayload {
        requested_asset: details.asset,
        callback: to_binary(&callback_msg)?,
    };

    // Call stablecoin Vault
    Ok(
        Response::new().add_message(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: deps.api.addr_humanize(&state.vault_address)?.to_string(),
            msg: to_binary(&VaultMsg::FlashLoan { payload })?,
            funds: vec![],
        })),
    )
}

// Attempt to perform an arbitrage operation with the assumption that
// the currency to be arb'd is below peg. Needed funds should be provided
// by the earlier stablecoin vault flashloan call.

pub fn try_arb_below_peg(
    deps: DepsMut,
    env: Env,
    msg_info: MessageInfo,
    details: ArbDetails,
) -> VaultResult {
    let state = STATE.load(deps.storage)?;
    let deposit_info = ARB_BASE_ASSET.load(deps.storage)?;

    // Ensure the caller is the vault
    if deps.api.addr_canonicalize(&msg_info.sender.to_string())? != state.vault_address {
        return Err(StableArbError::Unauthorized {});
    }

    // Set vars
    let denom = deposit_info.get_denom()?;
    let lent_coin = deduct_tax(
        deps.as_ref(),
        Coin::new(details.asset.amount.u128(), denom.clone()),
    )?;
    let ask_denom = LUNA_DENOM.to_string();
    let response: Response<TerraMsgWrapper> = Response::new();

    // Check if we have enough funds
    let balance = query_balance(&deps.querier, env.contract.address.clone(), denom)?;
    if balance < details.asset.amount {
        return Err(StableArbError::Broke {});
    }

    // Simulate first tx with Terra Market Module
    // lent_coin already takes transfer tax into account.
    let expected_luna_received =
        query_market_price(deps.as_ref(), lent_coin.clone(), ask_denom.clone())?;

    // Construct offer for Astroport
    let offer_coin = Coin {
        denom: ask_denom.clone(),
        amount: expected_luna_received,
    };

    // Market swap msg, swap STABLE -> LUNA
    let swap_msg = create_swap_msg(lent_coin.clone(), ask_denom);

    // Astroport msg, swap LUNA -> STABLE
    let astroport_msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: deps.api.addr_humanize(&state.pool_address)?.to_string(),
        funds: vec![offer_coin.clone()],
        msg: to_binary(&create_astroport_msg(
            offer_coin,
            details.belief_price,
            Some(details.slippage),
        ))?,
    });

    let logs = vec![
        ("action", String::from("arb below peg")),
        ("offer_amount", lent_coin.amount.to_string()),
        ("expected_luna", expected_luna_received.to_string()),
    ];

    // Create callback, this will send the funds back to the vault.
    let callback_msg =
        CallbackMsg::AfterSuccessfulTradeCallback {}.to_cosmos_msg(&env.contract.address)?;

    Ok(response
        .add_attributes(logs)
        .add_message(swap_msg)
        .add_message(astroport_msg)
        .add_message(callback_msg))
}

// Attempt to perform an arbitrage operation with the assumption that
// the currency to be arb'd is above peg. Needed funds should be provided
// by the earlier stablecoin vault flashloan call.
pub fn try_arb_above_peg(
    deps: DepsMut,
    env: Env,
    msg_info: MessageInfo,
    details: ArbDetails,
) -> VaultResult {
    let state = STATE.load(deps.storage)?;
    let deposit_info = ARB_BASE_ASSET.load(deps.storage)?;

    // Ensure the caller is the vault
    if deps.api.addr_canonicalize(&msg_info.sender.to_string())? != state.vault_address {
        return Err(StableArbError::Unauthorized {});
    }

    // Set vars
    let denom = deposit_info.get_denom()?;
    let lent_coin = deduct_tax(
        deps.as_ref(),
        Coin::new(details.asset.amount.u128(), denom.clone()),
    )?;
    let ask_denom = LUNA_DENOM.to_string();
    let response: Response<TerraMsgWrapper> = Response::new();

    // Check if we have enough funds
    let balance = query_balance(&deps.querier, env.contract.address.clone(), denom)?;
    if balance < details.asset.amount {
        return Err(StableArbError::Broke {});
    }
    // Simulate first tx with Astroport
    let expected_luna_received = simulate_astroport_swap(
        deps.as_ref(),
        deps.api.addr_humanize(&state.pool_address)?,
        lent_coin.clone(),
    )?;

    // Construct offer for Market Swap
    let offer_coin = Coin {
        denom: ask_denom,
        amount: expected_luna_received,
    };

    // Astroport msg, swap STABLE -> LUNA
    let astroport_msg: CosmosMsg<TerraMsgWrapper> = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: deps.api.addr_humanize(&state.pool_address)?.to_string(),
        funds: vec![lent_coin.clone()],
        msg: to_binary(&create_astroport_msg(
            lent_coin.clone(),
            details.belief_price,
            Some(details.slippage),
        ))?,
    });

    // Market swap msg, swap LUNA -> STABLE
    let swap_msg = create_swap_msg(offer_coin, lent_coin.denom);

    let logs = vec![
        ("action", String::from("arb above peg")),
        ("offer_amount", lent_coin.amount.to_string()),
        ("expected_luna", expected_luna_received.to_string()),
    ];

    // Create callback, this will send the funds back to the vault.
    let callback_msg =
        CallbackMsg::AfterSuccessfulTradeCallback {}.to_cosmos_msg(&env.contract.address)?;

    Ok(response
        .add_attributes(logs)
        .add_message(astroport_msg)
        .add_message(swap_msg)
        .add_message(callback_msg))
}

//----------------------------------------------------------------------------------------
//  CALLBACK FUNCTION HANDLERS
//----------------------------------------------------------------------------------------

// After the arb this function returns the funds to the vault.
fn after_successful_trade_callback(deps: DepsMut, env: Env) -> VaultResult {
    let state = STATE.load(deps.storage)?;
    let stable_denom = ARB_BASE_ASSET.load(deps.storage)?.get_denom()?;
    let stables_in_contract =
        query_balance(&deps.querier, env.contract.address, stable_denom.clone())?;

    // Send asset back to vault
    let repay_asset = Asset {
        info: AssetInfo::NativeToken {
            denom: stable_denom,
        },
        amount: stables_in_contract,
    };

    Ok(Response::new().add_message(CosmosMsg::Bank(BankMsg::Send {
        to_address: deps.api.addr_humanize(&state.vault_address)?.to_string(),
        amount: vec![repay_asset.deduct_tax(&deps.querier)?],
    })))
}

//----------------------------------------------------------------------------------------
//  GOVERNANCE CONTROLLED SETTERS
//----------------------------------------------------------------------------------------

pub fn set_vault_addr(deps: DepsMut, msg_info: MessageInfo, vault_address: String) -> VaultResult {
    // Only the admin should be able to call this
    ADMIN.assert_admin(deps.as_ref(), &msg_info.sender)?;

    let mut state = STATE.load(deps.storage)?;
    // Get the old vault
    let previous_vault = deps.api.addr_humanize(&state.vault_address)?.to_string();
    // Store the new vault addr
    state.vault_address = deps.api.addr_canonicalize(&vault_address)?;
    STATE.save(deps.storage, &state)?;
    // Respond and note the previous vault address
    Ok(Response::new()
        .add_attribute("new vault", vault_address)
        .add_attribute("previous vault", previous_vault))
}

//----------------------------------------------------------------------------------------
//  QUERY HANDLERS
//----------------------------------------------------------------------------------------

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Config {} => to_binary(&try_query_config(deps)?),
    }
}

pub fn try_query_config(deps: Deps) -> StdResult<ArbBaseAsset> {
    let info: ArbBaseAsset = ARB_BASE_ASSET.load(deps.storage)?;
    Ok(info)
}