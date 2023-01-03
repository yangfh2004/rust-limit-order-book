//! Primitive types and conversion methods.
use ethers::types::{H160, U256};

// local type alias
pub type Address = H160;
pub type Hash = String;
pub type Decimal = String;
// constants
const MIN_PRICE: f64 = 1e-18;

pub fn u256_to_decimal(from: &U256) -> Decimal {
    let float = from.low_u128() as f64;
    format!("{:.2}", float * MIN_PRICE)
}

pub fn decimal_to_u256(from: &Decimal) -> U256 {
    U256::from((from.parse::<f64>().unwrap() / MIN_PRICE) as u128)
}

#[derive(Debug)]
pub enum OrderStatus {
    Created,
    Filled,
    PartiallyFilled,
}


