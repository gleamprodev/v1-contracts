use cosmwasm_std::{to_binary, Addr, StdError, Uint128, SubMsg, WasmMsg, CosmosMsg};
use cosmwasm_std::testing::{mock_env, mock_info};

use white_whale::treasury::dapp_base::error::BaseDAppError;
use white_whale::treasury::dapp_base::msg::BaseExecuteMsg;
use white_whale::treasury::dapp_base::state::{ADMIN, BaseState, load_contract_addr, STATE};

use crate::contract::execute;
use crate::msg::ExecuteMsg;
use crate::error::AstroportError;
use crate::tests::common::{TEST_CREATOR, TRADER_CONTRACT, TREASURY_CONTRACT};
use crate::tests::base_mocks::mocks::{mock_add_to_address_book, mock_instantiate};
use crate::tests::mock_querier::mock_dependencies;
use white_whale_testing::dapp_base::common::{WHALE_TOKEN, WHALE_UST_PAIR, WHALE_UST_LP_TOKEN};

/**
 * BaseExecuteMsg::UpdateConfig
 */
#[test]
pub fn test_unsuccessfully_update_config_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    let env = mock_env();
    let msg = ExecuteMsg::Base(BaseExecuteMsg::UpdateConfig {
        treasury_address: None,
        trader: None,
    });

    let info = mock_info("unauthorized", &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg);

    match res {
        Err(AstroportError::BaseDAppError(BaseDAppError::Admin(_))) => (),
        Ok(_) => panic!("Should return unauthorized Error, Admin(NotAdmin)"),
        err => panic!("Should return unauthorized Error, Admin(NotAdmin) {:?}", err),
    }
}

#[test]
pub fn test_successfully_update_config_treasury_address_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    let env = mock_env();
    let msg = ExecuteMsg::Base(BaseExecuteMsg::UpdateConfig {
        treasury_address: Some("new_treasury_address".to_string()),
        trader: None,
    });

    let info = mock_info(TEST_CREATOR, &[]);
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    let state = STATE.load(deps.as_mut().storage).unwrap();

    assert_eq!(
        state,
        BaseState {
            treasury_address: Addr::unchecked("new_treasury_address".to_string()),
            trader: Addr::unchecked(TRADER_CONTRACT.to_string()),
        }
    )
}

#[test]
pub fn test_successfully_update_config_trader_address_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    let env = mock_env();
    let msg = ExecuteMsg::Base(BaseExecuteMsg::UpdateConfig {
        treasury_address: None,
        trader: Some("new_trader_address".to_string()),
    });

    let info = mock_info(TEST_CREATOR, &[]);
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    let state = STATE.load(deps.as_mut().storage).unwrap();

    assert_eq!(
        state,
        BaseState {
            treasury_address: Addr::unchecked(TREASURY_CONTRACT.to_string()),
            trader: Addr::unchecked("new_trader_address".to_string()),
        }
    )
}

#[test]
pub fn test_successfully_update_config_both_treasury_and_trader_address_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    let env = mock_env();
    let msg = ExecuteMsg::Base(BaseExecuteMsg::UpdateConfig {
        treasury_address: Some("new_treasury_address".to_string()),
        trader: Some("new_trader_address".to_string()),
    });

    let info = mock_info(TEST_CREATOR, &[]);
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    let state = STATE.load(deps.as_mut().storage).unwrap();

    assert_eq!(
        state,
        BaseState {
            treasury_address: Addr::unchecked("new_treasury_address".to_string()),
            trader: Addr::unchecked("new_trader_address".to_string()),
        }
    )
}

#[test]
pub fn test_successfully_update_config_none_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    let env = mock_env();
    let msg = ExecuteMsg::Base(BaseExecuteMsg::UpdateConfig {
        treasury_address: None,
        trader: None,
    });

    let info = mock_info(TEST_CREATOR, &[]);
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    let state = STATE.load(deps.as_mut().storage).unwrap();

    assert_eq!(
        state,
        BaseState {
            treasury_address: Addr::unchecked(TREASURY_CONTRACT.to_string()),
            trader: Addr::unchecked(TRADER_CONTRACT.to_string()),
        }
    )
}

/**
 * BaseExecuteMsg::SetAdmin
 */
#[test]
pub fn test_unsuccessfully_set_admin_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    let env = mock_env();
    let msg = ExecuteMsg::Base(BaseExecuteMsg::SetAdmin {
        admin: "new_admin".to_string(),
    });

    let info = mock_info("unauthorized", &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg);

    match res {
        Err(AstroportError::BaseDAppError(BaseDAppError::Admin(_))) => (),
        Ok(_) => panic!("Should return unauthorized Error, Admin(NotAdmin)"),
        _ => panic!("Should return unauthorized Error, Admin(NotAdmin)"),
    }
}

#[test]
pub fn test_successfully_set_admin_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    let env = mock_env();

    // check original admin
    let admin = ADMIN.get(deps.as_ref()).unwrap().unwrap();
    assert_eq!(admin, Addr::unchecked(TEST_CREATOR.to_string()));

    // set new admin
    let msg = ExecuteMsg::Base(BaseExecuteMsg::SetAdmin {
        admin: "new_admin".to_string(),
    });
    let info = mock_info(TEST_CREATOR, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    assert_eq!(0, res.messages.len());

    // check new admin
    let admin = ADMIN.get(deps.as_ref()).unwrap().unwrap();
    assert_eq!(admin, Addr::unchecked("new_admin".to_string()));
}

/**
 * BaseExecuteMsg::UpdateAddressBook
 */
#[test]
pub fn test_unsuccessfully_update_address_book_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    let env = mock_env();
    let msg = ExecuteMsg::Base(BaseExecuteMsg::UpdateAddressBook {
        to_add: vec![],
        to_remove: vec![],
    });

    let info = mock_info("unauthorized", &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg);

    match res {
        Err(AstroportError::BaseDAppError(BaseDAppError::Admin(_))) => (),
        Ok(_) => panic!("Should return unauthorized Error, Admin(NotAdmin)"),
        _ => panic!("Should return unauthorized Error, Admin(NotAdmin)"),
    }
}

#[test]
pub fn test_successfully_update_address_book_add_address_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    let env = mock_env();
    let msg = ExecuteMsg::Base(BaseExecuteMsg::UpdateAddressBook {
        to_add: vec![("asset".to_string(), "address".to_string())],
        to_remove: vec![],
    });

    let info = mock_info(TEST_CREATOR, &[]);
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    let asset_address = load_contract_addr(deps.as_ref(), "asset").unwrap();
    assert_eq!(asset_address, Addr::unchecked("address".to_string()));
}

#[test]
pub fn test_successfully_update_address_book_remove_address_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    let env = mock_env();

    // add address
    let msg = ExecuteMsg::Base(BaseExecuteMsg::UpdateAddressBook {
        to_add: vec![("asset".to_string(), "address".to_string())],
        to_remove: vec![],
    });

    let info = mock_info(TEST_CREATOR, &[]);
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    let asset_address = load_contract_addr(deps.as_ref(), "asset").unwrap();
    assert_eq!(asset_address, Addr::unchecked("address".to_string()));

    // remove address
    let msg = ExecuteMsg::Base(BaseExecuteMsg::UpdateAddressBook {
        to_add: vec![],
        to_remove: vec!["asset".to_string()],
    });

    let info = mock_info(TEST_CREATOR, &[]);
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    let res = load_contract_addr(deps.as_ref(), "asset");

    match res {
        Err(StdError::NotFound { .. }) => (),
        Ok(_) => panic!("Should return NotFound Err"),
        _ => panic!("Should return NotFound Err"),
    }
}


#[test]
pub fn test_successfully_update_address_book_add_and_removeaddress_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    let env = mock_env();

    //add address
    let msg = ExecuteMsg::Base(BaseExecuteMsg::UpdateAddressBook {
        to_add: vec![("asset".to_string(), "address".to_string())],
        to_remove: vec![],
    });

    let info = mock_info(TEST_CREATOR, &[]);
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    let asset_address = load_contract_addr(deps.as_ref(), "asset").unwrap();
    assert_eq!(asset_address, Addr::unchecked("address".to_string()));

    // query non-existing address
    let res = load_contract_addr(deps.as_ref(), "another_asset");
    match res {
        Err(StdError::NotFound { .. }) => (),
        Ok(_) => panic!("Should return NotFound Err"),
        _ => panic!("Should return NotFound Err"),
    }

    //add and remove addresses
    let msg = ExecuteMsg::Base(BaseExecuteMsg::UpdateAddressBook {
        to_add: vec![("another_asset".to_string(), "another_address".to_string())],
        to_remove: vec!["asset".to_string()],
    });
    let info = mock_info(TEST_CREATOR, &[]);
    execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    // another_asset should be in the addressbook now
    let asset_address = load_contract_addr(deps.as_ref(), "another_asset").unwrap();
    assert_eq!(asset_address, Addr::unchecked("another_address".to_string()));

    // asset should not be in the addressbook now
    let res = load_contract_addr(deps.as_ref(), "asset");
    match res {
        Err(StdError::NotFound { .. }) => (),
        Ok(_) => panic!("Should return NotFound Err"),
        _ => panic!("Should return NotFound Err"),
    }
}

/**
 * ExecuteMsg::ProvideLiquidity
 */
#[test]
pub fn test_provide_liquidity_unauthorized_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    let env = mock_env();
    let msg = ExecuteMsg::ProvideLiquidity {
        pool_id: "".to_string(),
        main_asset_id: "".to_string(),
        amount: Default::default(),
    };

    let info = mock_info("unauthorized", &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg);

    match res {
        Err(AstroportError::BaseDAppError(BaseDAppError::Unauthorized {})) => (),
        Ok(_) => panic!("Should return unauthorized Error, DAppError::Unauthorized"),
        _ => panic!("Should return unauthorized Error, DAppError::Unauthorized"),
    }
}

#[test]
pub fn test_successfully_provide_liquidity_nonexisting_asset_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());

    let env = mock_env();
    let msg = ExecuteMsg::ProvideLiquidity {
        pool_id: "asset".to_string(),
        main_asset_id: "".to_string(),
        amount: Default::default(),
    };

    let info = mock_info(TRADER_CONTRACT, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg);

    match res {
        Err(AstroportError::Std(_)) => (),
        Ok(_) => panic!("Should return NotFound Err"),
        _ => panic!("Should return NotFound Err"),
    }
}

#[test]
pub fn test_successfully_provide_liquidity_existing_asset_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    mock_add_to_address_book(deps.as_mut(), ("asset".to_string(), WHALE_TOKEN.to_string()));
    mock_add_to_address_book(deps.as_mut(), ("pool".to_string(), WHALE_UST_PAIR.to_string()));


    let env = mock_env();
    let msg = ExecuteMsg::ProvideLiquidity {
        pool_id: "pool".to_string(),
        main_asset_id: "asset".to_string(),
        amount: Default::default(),
    };

    let info = mock_info(TRADER_CONTRACT, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
}

#[test]
pub fn test_successfully_provide_detailed_liquidity_existing_asset_msg() {
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    mock_add_to_address_book(deps.as_mut(), ("asset".to_string(), WHALE_TOKEN.to_string()));
    mock_add_to_address_book(deps.as_mut(), ("pool".to_string(), WHALE_UST_PAIR.to_string()));


    let env = mock_env();
    let msg = ExecuteMsg::DetailedProvideLiquidity {
        pool_id: "pool".to_string(),
        assets: vec![("asset".to_string(), Uint128::from(10u64)), ("asset".to_string(), Uint128::from(10u64))],
        slippage_tolerance: Default::default(),
    };

    let info = mock_info(TRADER_CONTRACT, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
}

#[test]
/// Test to confirm that we can use DetailedProvideLiquidity to provide
/// some assets and then use WithdrawLiqudity to again withdraw those assets. 
/// The balances for WHALE_TOKEN and WHALE_UST_LP_TOKEN are mocked and do not reflect real values
/// Interactions of these dapps can be tested via integration tests
pub fn test_successfully_withdraw_liqudity(){
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    mock_add_to_address_book(deps.as_mut(), ("asset".to_string(), WHALE_TOKEN.to_string()));
    mock_add_to_address_book(deps.as_mut(), ("pool".to_string(), WHALE_UST_PAIR.to_string()));
    mock_add_to_address_book(deps.as_mut(), ("whale_ust".to_string(), WHALE_UST_LP_TOKEN.to_string()));
    mock_add_to_address_book(deps.as_mut(), ("whale_ust_pair".to_string(), WHALE_UST_PAIR.to_string()));


    let env = mock_env();
    let msg = ExecuteMsg::DetailedProvideLiquidity {
        pool_id: "pool".to_string(),
        assets: vec![("asset".to_string(), Uint128::from(10u64)), ("asset".to_string(), Uint128::from(10u64))],
        slippage_tolerance: Default::default(),
    };

    let info = mock_info(TRADER_CONTRACT, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();
    println!("{:?}", res.events);
    println!("{:?}", res.messages);

    let msg = ExecuteMsg::WithdrawLiquidity{
        lp_token_id: "whale_ust".to_string(),
        amount: Uint128::new(1),
    };
    let info = mock_info(TRADER_CONTRACT, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg).unwrap();

    assert_eq!(res.messages.len(), 1)
}

#[test]
pub fn test_successful_astro_swap(){
    let mut deps = mock_dependencies(&[]);
    mock_instantiate(deps.as_mut());
    mock_add_to_address_book(deps.as_mut(), ("asset".to_string(), WHALE_TOKEN.to_string()));
    mock_add_to_address_book(deps.as_mut(), ("pool".to_string(), WHALE_UST_PAIR.to_string()));
    mock_add_to_address_book(deps.as_mut(), ("whale_ust".to_string(), WHALE_UST_LP_TOKEN.to_string()));
    mock_add_to_address_book(deps.as_mut(), ("whale_ust_pair".to_string(), WHALE_UST_PAIR.to_string()));


    let env = mock_env();
    let msg = ExecuteMsg::SwapAsset {
        pool_id: "pool".to_string(),
        offer_id: "asset".to_string(),
        amount: Uint128::new(1),
        max_spread: None,
        belief_price: None,
    };

    let info = mock_info(TRADER_CONTRACT, &[]);
    let res = execute(deps.as_mut(), env.clone(), info, msg.clone()).unwrap();

    assert_eq!(res.messages.len(), 1);
}