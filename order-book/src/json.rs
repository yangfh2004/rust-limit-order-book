//! Basic data structures for JSON serialization.
use crate::primitive::{Address, Decimal, Hash};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Side {
    Bid = 0,
    Ask = 1,
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct JsonAccount {
    pub ddxBalance: Decimal,
    pub usdBalance: Decimal,
    pub traderAddress: Address,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct JsonOrder {
    pub amount: Decimal,
    pub nonce: Hash,
    pub price: Decimal,
    pub side: Side,
    pub traderAddress: Address,
}

// Implement `Display` for `JsonOrder`.
impl fmt::Display for JsonOrder {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Use `self.number` to refer to each positional data point.
        write!(f, "{}", serde_json::to_string(self).unwrap())
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonFill {
    pub(crate) maker_hash: Hash,
    pub(crate) taker_hash: Hash,
    pub(crate) fill_amount: Decimal,
    pub(crate) price: Decimal,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SimpleOrder {
    pub(crate) amount: Decimal,
    pub(crate) price: Decimal,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct L2OrderBook {
    pub(crate) asks: Vec<SimpleOrder>,
    pub(crate) bids: Vec<SimpleOrder>,
}

impl L2OrderBook {
    pub fn new() -> Self {
        Self {
            asks: Vec::new(),
            bids: Vec::new(),
        }
    }
}
