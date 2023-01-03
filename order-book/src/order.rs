//! Methods and structures for limit orders.
use crate::json::{JsonOrder, Side};
use crate::primitive::{Address, decimal_to_u256, Hash, u256_to_decimal};
use ethers::abi::AbiEncode;
use ethers::types::{transaction::eip712::Eip712, U256};
use ethers_contract::EthAbiType;
use ethers_derive_eip712::*;

impl JsonOrder {
    pub fn encode_order(&self) -> Order {
        // TODO: here may lose some precision
        let amount = decimal_to_u256(&self.amount);
        let price = decimal_to_u256(&self.price);
        let no_prefix = self.nonce.strip_prefix("0x").unwrap();
        let nonce = U256::from(hex::decode(no_prefix).unwrap().as_slice());
        let side: u8 = self.side.clone() as u8;
        Order {
            amount,
            nonce,
            price,
            side,
            traderAddress: self.traderAddress.clone(),
        }
    }

    pub fn get_trader(&self) -> String {
        format!("0x{}", self.traderAddress.encode_hex())
    }
}

/// Order structure for computing and EIP712 hashing.
#[derive(Debug, Copy, Clone, Eip712, EthAbiType)]
#[eip712(name = "DDX take-home", version = "0.1.0")]
#[allow(non_snake_case)]
pub struct Order {
    pub amount: U256,
    pub nonce: U256,
    pub price: U256,
    pub side: u8,
    pub traderAddress: Address,
}

impl Order {
    pub fn to_json(&self) -> JsonOrder {
        JsonOrder {
            amount: u256_to_decimal(&self.amount),
            nonce: format!("0x{}", &self.nonce.encode_hex()),
            price: u256_to_decimal(&self.price),
            side: self.get_side(),
            traderAddress: self.traderAddress.clone(),
        }
    }

    pub fn hash_hex(&self) -> Hash {
        let hash_bytes = self.encode_eip712().unwrap();
        let mut prefix = "0x".to_string();
        let hash_str = hex::encode(&hash_bytes);
        prefix.push_str(&hash_str);
        prefix
    }

    pub fn get_side(&self) -> Side {
        match self.side {
            0 => Side::Bid,
            _ => Side::Ask,
        }
    }
}
