use cosmwasm_std::StdError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Expired")]
    Expired {},

    #[error("Insufficient Nft Balance")]
    InsufficientNftBalance {},

    #[error("Insufficient funds")]
    InsufficientFundsSend {},

    #[error("Invalid auction")]
    InvalidAuction {},

    #[error("Auction Ended")]
    AuctionEnded {},
}
