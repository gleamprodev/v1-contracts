use cosmwasm_std::{DepsMut, MessageInfo, Response, StdResult};

use crate::treasury::dapp_base::common::BaseDAppResult;
use crate::treasury::dapp_base::msg::BaseExecuteMsg;
use crate::treasury::dapp_base::state::{ADDRESS_BOOK, ADMIN, STATE};

/// Handles the common base execute messages
pub fn handle_base_message(deps: DepsMut, info: MessageInfo, message: BaseExecuteMsg) -> BaseDAppResult {
    match message {
        BaseExecuteMsg::UpdateConfig {
            treasury_address,
            trader,
        } => update_config(deps, info, treasury_address, trader),
        BaseExecuteMsg::SetAdmin { admin } => set_admin(deps, info, admin),
        BaseExecuteMsg::UpdateAddressBook { to_add, to_remove } =>
            update_address_book(deps, info, to_add, to_remove)
    }
}

//----------------------------------------------------------------------------------------
//  GOVERNANCE CONTROLLED SETTERS
//----------------------------------------------------------------------------------------

/// Adds, updates or removes provided addresses. 
pub fn update_address_book(
    deps: DepsMut,
    msg_info: MessageInfo,
    to_add: Vec<(String, String)>,
    to_remove: Vec<String>,
) -> BaseDAppResult {
    // Only Admin can call this method
    ADMIN.assert_admin(deps.as_ref(), &msg_info.sender)?;

    for (name, new_address) in to_add.into_iter() {
        // Update function for new or existing keys
        let insert = |vault_asset: Option<String>| -> StdResult<String> {
            match vault_asset {
                Some(_) => Ok(new_address),
                None => Ok(new_address),
            }
        };
        ADDRESS_BOOK.update(deps.storage, name.as_str(), insert)?;
    }

    for name in to_remove {
        ADDRESS_BOOK.remove(deps.storage, name.as_str());
    }

    Ok(Response::new().add_attribute("action", "updated address book"))
}

/// Updates trader or treasury address
pub fn update_config(
    deps: DepsMut,
    info: MessageInfo,
    treasury_address: Option<String>,
    trader: Option<String>,
) -> BaseDAppResult {
    // Only the admin should be able to call this
    ADMIN.assert_admin(deps.as_ref(), &info.sender)?;

    let mut state = STATE.load(deps.storage)?;

    if let Some(treasury_address) = treasury_address {
        state.treasury_address = deps.api.addr_validate(treasury_address.as_str())?;
    }

    if let Some(trader) = trader {
        state.trader = deps.api.addr_validate(trader.as_str())?;
    }

    STATE.save(deps.storage, &state)?;
    Ok(Response::new().add_attribute("Update:", "Successful"))
}

pub fn set_admin(deps: DepsMut, info: MessageInfo, admin: String) -> BaseDAppResult {
    ADMIN.assert_admin(deps.as_ref(), &info.sender)?;

    let admin_addr = deps.api.addr_validate(&admin)?;
    let previous_admin = ADMIN.get(deps.as_ref())?.unwrap();
    ADMIN.execute_update_admin(deps, info, Some(admin_addr))?;
    Ok(Response::default()
        .add_attribute("previous admin", previous_admin)
        .add_attribute("admin", admin))
}