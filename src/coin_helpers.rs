use crate::error::ContractError;
use cosmwasm_std::{Coin, Uint128};

pub fn assert_sent_sufficient_coin(
    sent: &[Coin],
    required: Option<Coin>,
) -> Result<Option<Coin>, ContractError> {
    let mut sent_coin: Coin = Coin {
        denom: String::from("token"),
        amount: Uint128::from(0u64),
    };
    if let Some(required_coin) = required {
        let required_amount = required_coin.amount.u128();
        if required_amount > 0 {
            let sent_sufficient_funds = sent.iter().any(|coin| {
                // check if a given sent coin matches denom
                // and has sufficient amount
                sent_coin = coin.clone();
                coin.denom == required_coin.denom && coin.amount.u128() > required_amount
            });

            if sent_sufficient_funds {
                return Ok(Some(sent_coin));
            } else {
                return Err(ContractError::InsufficientFundsSend {});
            }
        }
    }

    Ok(Some(sent_coin))
}

#[cfg(test)]
mod test {
    use super::*;
    use cosmwasm_std::{coin, coins};

    #[test]
    fn assert_sent_sufficient_coin_works() {
        match assert_sent_sufficient_coin(&[], Some(coin(0, "token"))) {
            Ok(_) => {}
            Err(e) => panic!("Unexpected error: {:?}", e),
        };

        match assert_sent_sufficient_coin(&[], Some(coin(5, "token"))) {
            Ok(_) => panic!("Should have raised insufficient funds error"),
            Err(ContractError::InsufficientFundsSend {}) => {}
            Err(e) => panic!("Unexpected error: {:?}", e),
        };

        match assert_sent_sufficient_coin(&coins(10, "smokin"), Some(coin(5, "token"))) {
            Ok(_) => panic!("Should have raised insufficient funds error"),
            Err(ContractError::InsufficientFundsSend {}) => {}
            Err(e) => panic!("Unexpected error: {:?}", e),
        };

        match assert_sent_sufficient_coin(&coins(10, "token"), Some(coin(5, "token"))) {
            Ok(_) => {}
            Err(e) => panic!("Unexpected error: {:?}", e),
        };
    }
}