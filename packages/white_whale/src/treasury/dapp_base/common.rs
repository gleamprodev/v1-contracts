use cosmwasm_std::Response;
use crate::treasury::dapp_base::error::BaseDAppError;

/// Postfix for LP pair addresses. 
pub const PAIR_POSTFIX: &str = "_pair";
pub const ANCHOR_MONEY_MARKET_ID: &str = "anchor_money_market"
pub type BaseDAppResult = Result<Response, BaseDAppError>;
