//! Fill results for the limit order match engine.
use crate::json::{JsonFill, Side};
use crate::primitive::{Address, Hash, OrderStatus, u256_to_decimal};
use ethers::types::U256;

#[derive(Debug, Clone)]
pub struct Fill {
    pub(crate) from: Address,
    pub(crate) to: Address,
    pub(crate) maker_hash: Hash,
    pub(crate) taker_hash: Hash,
    pub(crate) fill_amount: U256,
    pub(crate) price: U256,
}

#[derive(Debug)]
pub struct FillResult {
    pub filled_orders: Vec<Fill>,
    pub remaining: U256,
    pub status: OrderStatus,
    pub side: Side,
}

impl FillResult {
    pub fn new(remaining: U256, side: Side) -> Self {
        FillResult {
            filled_orders: Vec::new(),
            remaining,
            status: OrderStatus::Created,
            side,
        }
    }
    pub fn generate_filled_orders(&self) -> Vec<JsonFill> {
        let mut filled_orders = Vec::new();
        for fill in &self.filled_orders {
            let json_fill = JsonFill {
                maker_hash: fill.maker_hash.clone(),
                taker_hash: fill.taker_hash.clone(),
                fill_amount: u256_to_decimal(&fill.fill_amount),
                price: u256_to_decimal(&fill.price),
            };
            filled_orders.push(json_fill);
        }
        filled_orders
    }
}
