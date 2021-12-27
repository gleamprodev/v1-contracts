#![allow(unused_imports)]
#![allow(unused_variables)]

use cosmwasm_std::{Binary, Deps, DepsMut, entry_point, Env, MessageInfo, Response, StdResult};

use white_whale::treasury::dapp_base::commands as dapp_base_commands;
use white_whale::treasury::dapp_base::common::BaseDAppResult;
use white_whale::treasury::dapp_base::msg::BaseInstantiateMsg;
use white_whale::treasury::dapp_base::queries as dapp_base_queries;
use white_whale::treasury::dapp_base::state::{ADMIN, BaseState, STATE};

use crate::commands;
use crate::msg::{ExecuteMsg, QueryMsg};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: BaseInstantiateMsg,
) -> BaseDAppResult {
    let state = BaseState {
        treasury_address: deps.api.addr_validate(&msg.treasury_address)?,
        trader: deps.api.addr_validate(&msg.trader)?,
        memory_addr: deps.api.addr_validate(&msg.memory_addr)?,
    };

    STATE.save(deps.storage, &state)?;
    ADMIN.set(deps, Some(info.sender))?;

    Ok(Response::default())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(deps: DepsMut, env: Env, info: MessageInfo, msg: ExecuteMsg) -> BaseDAppResult {
    match msg {
        ExecuteMsg::Base(message) => dapp_base_commands::handle_base_message(deps, info, message),
        // handle dapp-specific messages here
        // ExecuteMsg::Custom{} => commands::custom_command(),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::Base(message) => dapp_base_queries::handle_base_query(deps, message),
        // handle dapp-specific queries here
        // QueryMsg::Custom{} => queries::custom_query(),
    }
}
