use cosmwasm_std::{
    entry_point, to_binary, Binary, CosmosMsg, Deps, DepsMut, Env, MessageInfo, Reply, ReplyOn,
    Response, StdResult, SubMsg, Uint128, WasmMsg,
};
use cw20::Cw20ExecuteMsg;
use terraswap::asset::{Asset, AssetInfo};
use terraswap::pair::ExecuteMsg as PairExecuteMsg;
use terraswap::querier::{query_balance, query_token_balance};

use white_whale::community_fund::msg::{ConfigResponse, ExecuteMsg, QueryMsg};
use white_whale::denom::{UST_DENOM, WHALE_DENOM};
use white_whale::msg::AnchorMsg;
use white_whale::query::anchor::query_aust_exchange_rate;

use crate::error::CommunityFundError;
use crate::msg::InstantiateMsg;
use crate::state::{State, ADMIN, STATE};

/*
    The Community fund holds the protocol treasury and has control over the protocol owned liquidity.
    It is controlled by the governance contract and serves to grow its holdings and give grants to proposals.
*/

type CommunityFundResult = Result<Response, CommunityFundError>;

pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> StdResult<Response> {
    deps.api.addr_validate(&msg.whale_token_addr)?;

    let state = State {
        whale_token_addr: deps.api.addr_canonicalize(&msg.whale_token_addr)?,
    };

    STATE.save(deps.storage, &state)?;
    ADMIN.set(deps, Some(info.sender))?;

    Ok(Response::default())
}

pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> CommunityFundResult {
    match msg {
        ExecuteMsg::Spend { recipient, amount } => {
            spend_whale(deps.as_ref(), info, recipient, amount)
        }
        ExecuteMsg::Burn { amount } => burn_whale(deps.as_ref(), info, amount),
        ExecuteMsg::Deposit {} => deposit(deps, &env, info),
        ExecuteMsg::SetAdmin { admin } => {
            let admin_addr = deps.api.addr_validate(&admin)?;
            let previous_admin = ADMIN.get(deps.as_ref())?.unwrap();
            ADMIN.execute_update_admin(deps, info, Some(admin_addr))?;
            Ok(Response::default()
                .add_attribute("previous admin", previous_admin)
                .add_attribute("admin", admin))
        }
    }
}

// Transfer WHALE to specified recipient
pub fn spend_whale(
    deps: Deps,
    info: MessageInfo,
    recipient: String,
    amount: Uint128,
) -> CommunityFundResult {
    ADMIN.assert_admin(deps, &info.sender)?;
    let state = STATE.load(deps.storage)?;
    Ok(
        Response::new().add_message(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: deps.api.addr_humanize(&state.whale_token_addr)?.to_string(),
            funds: vec![],
            msg: to_binary(&Cw20ExecuteMsg::Transfer { recipient, amount })?,
        })),
    )
}

// Call burn on WHALE cw20 token
pub fn burn_whale(deps: Deps, info: MessageInfo, amount: Uint128) -> CommunityFundResult {
    ADMIN.assert_admin(deps, &info.sender)?;
    let state = STATE.load(deps.storage)?;

    Ok(
        Response::new().add_message(CosmosMsg::Wasm(WasmMsg::Execute {
            contract_addr: deps.api.addr_humanize(&state.whale_token_addr)?.to_string(),
            funds: vec![],
            msg: to_binary(&Cw20ExecuteMsg::Burn { amount })?,
        })),
    )
}

// Deposits WHALE tokens into the contract
pub fn deposit(deps: DepsMut, env: &Env, msg_info: MessageInfo) -> CommunityFundResult {
    if msg_info.funds.len() > 1 {
        return Err(CommunityFundError::WrongDepositTooManyTokens {});
    } else if msg_info.funds.first()?.denom != WHALE_DENOM {
        return Err(CommunityFundError::WrongDepositToken {});
    }

    let mut state = STATE.load(deps.storage)?;

    let msg = CosmosMsg::Wasm(WasmMsg::Execute {
        contract_addr: state.whale_token_addr.to_string(),
        funds: vec![],
        msg: to_binary(&Cw20ExecuteMsg::TransferFrom {
            owner: msg_info.sender.to_string(),
            recipient: env.contract.address.to_string(),
            amount: msg_info.funds.first()?.amount,
        })?,
    });

    Ok(Response::new().add_message(msg))
}

pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Admin {} => Ok(to_binary(&ADMIN.query_admin(deps)?)?),
        QueryMsg::Config {} => query_config(deps),
    }
}

pub fn query_config(deps: Deps) -> StdResult<Binary> {
    let state = STATE.load(deps.storage)?;
    to_binary(&ConfigResponse {
        token_addr: deps.api.addr_humanize(&state.whale_token_addr)?,
    })
}