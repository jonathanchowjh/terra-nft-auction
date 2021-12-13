#[cfg(not(feature = "library"))]
use cosmwasm_std::entry_point;
use cosmwasm_std::{
    to_binary, Binary, Deps, DepsMut, Env,
    MessageInfo, Response, StdResult, Uint128, Addr, SubMsg,
    Coin, BankMsg, CosmosMsg, StdError
};
use cw2::set_contract_version;
use cw0::{Event,Expiration};

use crate::error::ContractError;
use crate::msg::{
    CountResponse, BalanceResponse, IsApprovedForAllResponse, ExecuteMsg, InstantiateMsg,
    QueryMsg, TokenId, Cw1155ReceiveMsg, AuctionResponse
};
use crate::state::{State, STATE, APPROVES, BALANCES, MINTER, TOKENS, Auction, AUCTIONS};
use crate::event::{TransferEvent,ApproveAllEvent};
use crate::coin_helpers::assert_sent_sufficient_coin;

// version info for migration info
const CONTRACT_NAME: &str = "crates.io:nft-auction";
const CONTRACT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    msg: InstantiateMsg,
) -> Result<Response, ContractError> {
    let state = State {
        count: msg.count,
        owner: info.sender.clone(),
    };
    set_contract_version(deps.storage, CONTRACT_NAME, CONTRACT_VERSION)?;
    STATE.save(deps.storage, &state)?;
    
    let minter = deps.api.addr_validate(&msg.minter)?;
    MINTER.save(deps.storage, &minter)?;

    Ok(Response::new()
        .add_attribute("method", "instantiate")
        .add_attribute("owner", info.sender)
        .add_attribute("count", msg.count.to_string())
        .add_attribute("minter", minter.to_string()))
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: ExecuteMsg,
) -> Result<Response, ContractError> {
    match msg {
        ExecuteMsg::Increment {} => try_increment(deps),
        ExecuteMsg::Reset { count } => try_reset(deps, info, count),

        ExecuteMsg::Mint { to, token_id, value, msg } => execute_mint(deps, env, info, to, token_id, value, msg),
        ExecuteMsg::SendFrom { from, to, token_id, value, msg } => execute_send_from(deps, env, info, from, to, token_id, value, msg),
        ExecuteMsg::ApproveAll { operator, expires } => execute_approve_all(deps, env, info, operator, expires),
        ExecuteMsg::RevokeAll { operator } => execute_revoke_all(deps, env, info, operator),

        ExecuteMsg::CreateAuction {
            token_id, amount, price, seller, bidding_close
        } => execute_create_auction(deps, env, info, token_id, amount, price, seller, bidding_close),
        ExecuteMsg::Bid { token_id, seller } => execute_bid(deps, env, info, token_id, seller),
        ExecuteMsg::CloseAuction { token_id, seller } => execute_auction_close(deps, env, info, token_id, seller),
    }
}

pub fn try_increment(deps: DepsMut) -> Result<Response, ContractError> {
    STATE.update(deps.storage, |mut state| -> Result<_, ContractError> {
        state.count += 1;
        Ok(state)
    })?;

    Ok(Response::new().add_attribute("method", "try_increment"))
}
pub fn try_reset(deps: DepsMut, info: MessageInfo, count: i32) -> Result<Response, ContractError> {
    STATE.update(deps.storage, |mut state| -> Result<_, ContractError> {
        if info.sender != state.owner {
            return Err(ContractError::Unauthorized {});
        }
        state.count = count;
        Ok(state)
    })?;
    Ok(Response::new().add_attribute("method", "reset"))
}

/// Check if such auction exist
/// Check if user has sufficient tokens
/// Create auction
pub fn execute_create_auction(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    token_id: TokenId,
    amount: Uint128,
    price: Coin,
    seller: String,
    bidding_close: Expiration,
) -> Result<Response, ContractError> {
    // Fetch Address and Balance
    let seller_addr = deps.api.addr_validate(&seller)?;
    let balance = BALANCES.may_load(deps.storage, (&info.sender, &token_id))?;
    match balance {
        None => { return Err(ContractError::InsufficientNftBalance {}); }
        Some(balance_val) => {
            // sufficient nft balance
            if balance_val < amount {
                return Err(ContractError::InsufficientNftBalance {});
            }
        }
    }
    // Fetch Auction
    let auction = AUCTIONS.may_load(deps.storage, (&seller_addr, &token_id))?;
    match auction {
        None => {
            // Create New Auction
            let new_auction = Auction {
                amount: amount,
                price: price,
                highest_bidder: seller_addr.clone(),
                bidding_close: bidding_close,
            };
            AUCTIONS.save(
                deps.storage,
                (&seller_addr, &token_id),
                &new_auction
            )?;
        }
        Some(_auction_val) => { return Err(ContractError::InvalidAuction {}); }
    }
    Ok(Response::new().add_attribute("method", "execute_create_auction"))
}

/// Get Auction Highest Bidder
/// Reject if price lower than highest
/// Set new price and owner
/// Return money to highest bidder
/// - Send Money Function
/// - Validate Money function
pub fn execute_bid(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    token_id: TokenId,
    seller: String,
) -> Result<Response, ContractError> {
    // Fetch auction and seller address
    let seller_addr = deps.api.addr_validate(&seller)?;
    let auction = AUCTIONS.may_load(deps.storage, (&seller_addr, &token_id))?;
    
    match auction {
        None => { return Err(ContractError::InvalidAuction {}); }
        Some(auction_val) => {
            // Bidding Not Expired
            if auction_val.bidding_close.is_expired(&env.block) {
                return Err(ContractError::AuctionEnded {});
            }
            // Sufficient coins
            let sent_coin = assert_sent_sufficient_coin(&info.funds, Some(auction_val.price.clone()))?;
            match sent_coin {
                None => { return Err(ContractError::InsufficientFundsSend {}); }
                Some(sent_coin_val) => {
                    let new_auction = Auction {
                        amount: auction_val.amount.clone(),
                        bidding_close: auction_val.bidding_close.clone(),
                        price: sent_coin_val,
                        highest_bidder: info.sender,
                    };
                    AUCTIONS.save(
                        deps.storage,
                        (&seller_addr, &token_id),
                        &new_auction
                    )?;
                }
            }
            Ok(Response::new()
                .add_attribute("Bidding", &token_id.to_string())
                .add_message(CosmosMsg::Bank(BankMsg::Send {
                    to_address: auction_val.highest_bidder.to_string(),
                    amount: vec![auction_val.price.clone()],
                })))
        }
    }
}

/// Check if Auction expired
/// Send NFT to highest bidder
/// Send Bid amount to user
pub fn execute_auction_close(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    token_id: String,
    seller: String,
) -> Result<Response, ContractError> {
    // Fetch Address and Auction
    let seller_addr = deps.api.addr_validate(&seller)?;
    let auction = AUCTIONS.may_load(deps.storage, (&seller_addr, &token_id))?;
    match auction {
        None => { return Err(ContractError::InvalidAuction {}); }
        Some(auction_val) => {
            // Send NFT to Highest Bidder
            execute_send_from(
                deps,
                env,
                info,
                seller_addr.to_string(),
                auction_val.highest_bidder.clone().to_string(),
                token_id.clone(),
                auction_val.amount.clone(),
                None,
            )?;
            // Send Money to Auction Seller
            Ok(Response::new()
                .add_attribute("Bidding", &token_id.to_string())
                .add_message(CosmosMsg::Bank(BankMsg::Send {
                    to_address: seller_addr.to_string(),
                    amount: vec![auction_val.price.clone()],
                })))
        }
    }
}


/// When from is None: mint new coins
/// When to is None: burn coins
/// When both are None: no token balance is changed, pointless but valid
/// Make sure permissions are checked before calling this.
fn execute_transfer_inner<'a>(
    deps: &'a mut DepsMut,
    from: Option<&'a Addr>,
    to: Option<&'a Addr>,
    token_id: &'a str,
    amount: Uint128,
) -> Result<TransferEvent<'a>, ContractError> {
    if let Some(from_addr) = from {
        BALANCES.update(
            deps.storage,
            (from_addr, token_id),
            |balance: Option<Uint128>| -> StdResult<_> {
                Ok(balance.unwrap_or_default().checked_sub(amount)?)
            },
        )?;
    }

    if let Some(to_addr) = to {
        BALANCES.update(
            deps.storage,
            (to_addr, token_id),
            |balance: Option<Uint128>| -> StdResult<_> {
                Ok(balance.unwrap_or_default().checked_add(amount)?)
            },
        )?;
    }

    Ok(TransferEvent {
        from: from.map(|x| x.as_ref()),
        to: to.map(|x| x.as_ref()),
        token_id,
        amount,
    })
}

/// returns true iff the sender can execute approve or reject on the contract
fn check_can_approve(deps: Deps, env: &Env, owner: &Addr, operator: &Addr) -> StdResult<bool> {
    // owner can approve
    if owner == operator {
        return Ok(true);
    }
    // operator can approve
    let op = APPROVES.may_load(deps.storage, (&owner, &operator))?;
    Ok(match op {
        Some(ex) => !ex.is_expired(&env.block),
        None => false,
    })
}

fn guard_can_approve(
    deps: Deps,
    env: &Env,
    owner: &Addr,
    operator: &Addr,
) -> Result<(), ContractError> {
    if !check_can_approve(deps, env, owner, operator)? {
        Err(ContractError::Unauthorized {})
    } else {
        Ok(())
    }
}

pub fn execute_mint(
    mut deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    to: String,
    token_id: TokenId,
    amount: Uint128,
    msg: Option<Binary>,
) -> Result<Response, ContractError> {
    let to_addr = deps.api.addr_validate(&to)?;

    if info.sender != MINTER.load(deps.storage)? {
        return Err(ContractError::Unauthorized {});
    }

    let mut rsp = Response::default();

    let event = execute_transfer_inner(&mut deps, None, Some(&to_addr), &token_id, amount)?;
    event.add_attributes(&mut rsp);

    if let Some(msg) = msg {
        rsp.messages = vec![SubMsg::new(
            Cw1155ReceiveMsg {
                operator: info.sender.to_string(),
                from: None,
                amount,
                token_id: token_id.clone(),
                msg,
            }
            .into_cosmos_msg(to)?,
        )]
    }

    // insert if not exist
    if !TOKENS.has(deps.storage, &token_id) {
        // we must save some valid data here
        TOKENS.save(deps.storage, &token_id, &String::new())?;
    }
    Ok(rsp)
}

pub fn execute_send_from(
    mut deps: DepsMut,
    env: Env,
    info: MessageInfo,
    from: String,
    to: String,
    token_id: TokenId,
    amount: Uint128,
    msg: Option<Binary>,
) -> Result<Response, ContractError> {
    let from_addr = deps.api.addr_validate(&from)?;
    let to_addr = deps.api.addr_validate(&to)?;

    guard_can_approve(deps.as_ref(), &env, &from_addr, &info.sender)?;

    let mut rsp = Response::default();

    let event = execute_transfer_inner(
        &mut deps,
        Some(&from_addr),
        Some(&to_addr),
        &token_id,
        amount,
    )?;
    event.add_attributes(&mut rsp);

    if let Some(msg) = msg {
        rsp.messages = vec![SubMsg::new(
            Cw1155ReceiveMsg {
                operator: info.sender.to_string(),
                from: Some(from),
                amount,
                token_id: token_id.clone(),
                msg,
            }
            .into_cosmos_msg(to)?,
        )]
    }
    Ok(rsp)
}

pub fn execute_approve_all(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    operator: String,
    expires: Option<Expiration>,
) -> Result<Response, ContractError> {

    // reject expired data as invalid
    let expires = expires.unwrap_or_default();
    if expires.is_expired(&env.block) {
        return Err(ContractError::Expired {});
    }

    // set the operator for us
    let operator_addr = deps.api.addr_validate(&operator)?;
    APPROVES.save(deps.storage, (&info.sender, &operator_addr), &expires)?;

    let mut rsp = Response::default();
    ApproveAllEvent {
        sender: info.sender.as_ref(),
        operator: &operator,
        approved: true,
    }
    .add_attributes(&mut rsp);
    Ok(rsp)
}

pub fn execute_revoke_all(
    deps: DepsMut,
    _env: Env,
    info: MessageInfo,
    operator: String
) -> Result<Response, ContractError> {
    let operator_addr = deps.api.addr_validate(&operator)?;
    APPROVES.remove(deps.storage, (&info.sender, &operator_addr));

    let mut rsp = Response::default();
    ApproveAllEvent {
        sender: info.sender.as_ref(),
        operator: &operator,
        approved: false,
    }
    .add_attributes(&mut rsp);
    Ok(rsp)
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, env: Env, msg: QueryMsg) -> StdResult<Binary> {
    match msg {
        QueryMsg::GetCount {} => to_binary(&query_count(deps)?),
        QueryMsg::Balance { owner, token_id } => {
            let owner_addr = deps.api.addr_validate(&owner)?;
            let balance = BALANCES
                .may_load(deps.storage, (&owner_addr, &token_id))?
                .unwrap_or_default();
            to_binary(&BalanceResponse { balance })
        },
        QueryMsg::IsApprovedForAll { owner, operator } => {
            let owner_addr = deps.api.addr_validate(&owner)?;
            let operator_addr = deps.api.addr_validate(&operator)?;
            let approved = check_can_approve(deps, &env, &owner_addr, &operator_addr)?;
            to_binary(&IsApprovedForAllResponse { approved })
        },
        QueryMsg::Auction { seller, token_id } => {
            let seller_addr = deps.api.addr_validate(&seller)?;
            let auction = AUCTIONS
                .may_load(deps.storage, (&seller_addr, &token_id))?;
            match auction {
                None => {
                    return Err(StdError::NotFound { kind: "invalid auction".to_string() });
                }
                Some(auction_val) => { return to_binary(&AuctionResponse { auction: auction_val }) }
            }
            
        },
    }
}

fn query_count(deps: Deps) -> StdResult<CountResponse> {
    let state = STATE.load(deps.storage)?;
    Ok(CountResponse { count: state.count })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmwasm_std::testing::{mock_dependencies, mock_env, mock_info};
    use cosmwasm_std::{coin, coins, from_binary};

    #[test]
    fn proper_initialization() {
        let mut deps = mock_dependencies(&[]);

        let msg = InstantiateMsg { count: 17, minter: "terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd8".to_string() };
        let info = mock_info("creator", &coins(1000, "ust"));

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // it worked, let's query the state
        let res = query(deps.as_ref(), mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(17, value.count);
    }
    /// Instantiate
    /// Mint Token
    /// Create Auction
    /// Bid Auction
    /// Close Auction
    #[test]
    fn auction() {
        let mut deps = mock_dependencies(&[]);

        let msg = InstantiateMsg { count: 17, minter: "terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd8".to_string() };
        let info = mock_info("creator", &coins(1000, "uusd"));

        // we can just call .unwrap() to assert this was a success
        let res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();
        assert_eq!(0, res.messages.len());

        // mint token
        // ===================
        let info = mock_info("terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd8", &coins(1000, "uusd"));
        let msg = ExecuteMsg::Mint {
            to: "terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd8".to_string(),
            token_id: "ID1".to_string(),
            value: Uint128::new(10001u128),
            msg: None,
        };
        let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        // query nft token
        let res = query(deps.as_ref(), mock_env(), QueryMsg::Balance {
            owner: "terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd8".to_string(),
            token_id: "ID1".to_string()
        }).unwrap();
        let value: BalanceResponse = from_binary(&res).unwrap();
        assert_eq!(Uint128::new(10001u128), value.balance);

        // create auction
        // ===================
        let info = mock_info("terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd8", &coins(1000, "uusd"));
        let msg = ExecuteMsg::CreateAuction {
            token_id: "ID1".to_string(),
            amount: Uint128::new(1u128),
            price: coin(1000, "uusd"),
            seller: "terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd8".to_string(),
            bidding_close: Expiration::AtHeight(23123),
        };
        let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        // query auction
        let res = query(deps.as_ref(), mock_env(), QueryMsg::Auction {
            seller: "terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd8".to_string(),
            token_id: "ID1".to_string()
        }).unwrap();
        let value: AuctionResponse = from_binary(&res).unwrap();
        assert_eq!(Uint128::new(1u128), value.auction.amount);
        assert_eq!(coin(1000, "uusd"), value.auction.price);
        assert_eq!(Expiration::AtHeight(23123), value.auction.bidding_close);

        // place bid
        // ===================
        let info = mock_info("terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd9", &coins(10000, "uusd"));
        let msg = ExecuteMsg::Bid {
            token_id: "ID1".to_string(),
            seller: "terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd8".to_string(),
        };
        let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        // query auction
        let res = query(deps.as_ref(), mock_env(), QueryMsg::Auction {
            seller: "terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd8".to_string(),
            token_id: "ID1".to_string()
        }).unwrap();
        let value: AuctionResponse = from_binary(&res).unwrap();
        assert_eq!(coin(10000, "uusd"), value.auction.price);

        // close auction
        // ===================
        let info = mock_info("terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd8", &coins(10000, "uusd"));
        let msg = ExecuteMsg::CloseAuction {
            token_id: "ID1".to_string(),
            seller: "terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd8".to_string(),
        };
        let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        // query nft token
        let res = query(deps.as_ref(), mock_env(), QueryMsg::Balance {
            owner: "terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd9".to_string(),
            token_id: "ID1".to_string()
        }).unwrap();
        let value: BalanceResponse = from_binary(&res).unwrap();
        assert_eq!(Uint128::new(1u128), value.balance);
    }

    #[test]
    fn increment() {
        let mut deps = mock_dependencies(&coins(2, "uusd"));

        let msg = InstantiateMsg { count: 17, minter: "terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd8".to_string() };
        let info = mock_info("creator", &coins(2, "uusd"));
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        // beneficiary can release it
        let info = mock_info("anyone", &coins(2, "uusd"));
        let msg = ExecuteMsg::Increment {};
        let _res = execute(deps.as_mut(), mock_env(), info, msg).unwrap();

        // should increase counter by 1
        let res = query(deps.as_ref(), mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(18, value.count);
    }

    #[test]
    fn reset() {
        let mut deps = mock_dependencies(&coins(2, "uusd"));

        let msg = InstantiateMsg { count: 17, minter: "terra1dcegyrekltswvyy0xy69ydgxn9x8x32zdtapd8".to_string() };
        let info = mock_info("creator", &coins(2, "uusd"));
        let _res = instantiate(deps.as_mut(), mock_env(), info, msg).unwrap();

        // beneficiary can release it
        let unauth_info = mock_info("anyone", &coins(2, "uusd"));
        let msg = ExecuteMsg::Reset { count: 5 };
        let res = execute(deps.as_mut(), mock_env(), unauth_info, msg);
        match res {
            Err(ContractError::Unauthorized {}) => {}
            _ => panic!("Must return unauthorized error"),
        }

        // only the original creator can reset the counter
        let auth_info = mock_info("creator", &coins(2, "uusd"));
        let msg = ExecuteMsg::Reset { count: 5 };
        let _res = execute(deps.as_mut(), mock_env(), auth_info, msg).unwrap();

        // should now be 5
        let res = query(deps.as_ref(), mock_env(), QueryMsg::GetCount {}).unwrap();
        let value: CountResponse = from_binary(&res).unwrap();
        assert_eq!(5, value.count);
    }
}
