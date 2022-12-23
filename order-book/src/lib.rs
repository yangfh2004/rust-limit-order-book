use ethers::types::{transaction::eip712::Eip712, H160, U256};
use ethers_contract::EthAbiType;
use ethers_derive_eip712::*;
use hex;
use log::debug;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ops::Div;

// local type alias
type Address = H160;
type Hash = String;
type Decimal = String;
// constants
const ORDER_BOOK_INIT_CAP: usize = 50_000;
const MIN_PRICE: f64 = 1e-18;
const L2_MAX: usize = 50;
const ERROR: u8 = 100;

fn u256_to_decimal(from: &U256) -> Decimal {
    let float = from.low_u64() as f64;
    (float * MIN_PRICE).to_string()
}

fn decimal_to_u256(from: &Decimal) -> U256 {
    U256::from((from.parse::<f64>().unwrap() / MIN_PRICE) as u64)
}

#[derive(Debug, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct JsonAccount {
    ddxBalance: Decimal,
    usdBalance: Decimal,
    traderAddress: Address,
}

#[derive(Debug)]
pub struct Account {
    ddx_balance: U256,
    usd_balance: U256,
    trader_address: Address,
}

impl Account {
    pub fn from_json(json: JsonAccount) -> Self {
        Self {
            ddx_balance: decimal_to_u256(&json.ddxBalance),
            usd_balance: decimal_to_u256(&json.usdBalance),
            trader_address: json.traderAddress.clone(),
        }
    }

    pub fn update(&mut self, side: Side, fill: Fill) {
        let unit_scale = U256::from(1e18 as u64);
        match side {
            Side::Bid => {
                self.ddx_balance += fill.fill_amount;
                self.usd_balance -= fill
                    .fill_amount
                    .saturating_mul(fill.price)
                    .div(unit_scale);
            }
            Side::Ask => {
                self.ddx_balance -= fill.fill_amount;
                self.usd_balance += fill
                    .fill_amount
                    .saturating_mul(fill.price)
                    .div(unit_scale);
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Side {
    Bid,
    Ask,
}

#[derive(Debug)]
pub enum OrderStatus {
    Created,
    Filled,
    PartiallyFilled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(non_snake_case)]
pub struct JsonOrder {
    amount: Decimal,
    nonce: Hash,
    price: Decimal,
    side: Side,
    traderAddress: Address,
}

impl JsonOrder {
    pub fn encode_order(&self) -> Order {
        // TODO: here may lose some precision
        let amount = decimal_to_u256(&self.amount);
        let price = decimal_to_u256(&self.price);
        let nonce = U256::from(hex::decode(self.nonce.clone()).unwrap().as_slice());
        let side: u8 = match self.side {
            Side::Bid => 0,
            Side::Ask => 1,
        };
        Order {
            amount,
            nonce,
            price,
            side,
            traderAddress: self.traderAddress.clone(),
        }
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

#[derive(Debug, Serialize, Deserialize)]
pub struct SimpleOrder {
    amount: Decimal,
    price: Decimal,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct L2OrderBook {
    asks: Vec<SimpleOrder>,
    bids: Vec<SimpleOrder>,
}

impl L2OrderBook {
    pub fn new() -> Self {
        Self {
            asks: Vec::new(),
            bids: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonFill {
    maker_hash: Hash,
    taker_hash: Hash,
    fill_amount: Decimal,
    price: Decimal,
}

#[derive(Debug, Clone)]
pub struct Fill {
    maker_hash: Hash,
    taker_hash: Hash,
    fill_amount: U256,
    price: U256,
}

#[derive(Debug)]
pub struct FillResult {
    pub filled_orders: Vec<Fill>,
    pub remaining: U256,
    pub status: OrderStatus,
    pub side: Side,
}

impl FillResult {
    fn new(remaining: U256, side: Side) -> Self {
        FillResult {
            filled_orders: Vec::new(),
            remaining,
            status: OrderStatus::Created,
            side,
        }
    }
}

#[derive(Debug)]
struct HalfBook {
    side: Side,
    price_map: BTreeMap<U256, usize>,
    price_levels: Vec<VecDeque<(Hash, Order)>>,
}

impl HalfBook {
    pub fn new(side: Side) -> Self {
        HalfBook {
            side,
            price_map: BTreeMap::new(),
            price_levels: Vec::with_capacity(ORDER_BOOK_INIT_CAP),
        }
    }
}

#[derive(Debug)]
pub struct OrderBook {
    symbol: String,
    bid_book: HalfBook,
    ask_book: HalfBook,
    // For fast cancels Order Hash -> (Side, Price_level)
    order_loc: HashMap<Hash, (Side, usize)>,
}

impl OrderBook {
    pub fn new(symbol: String) -> Self {
        OrderBook {
            symbol,
            bid_book: HalfBook::new(Side::Bid),
            ask_book: HalfBook::new(Side::Ask),
            order_loc: HashMap::with_capacity(ORDER_BOOK_INIT_CAP),
        }
    }

    pub fn cancel_order(&mut self, order_id: Hash) -> Result<&str, &str> {
        if let Some((side, price_level)) = self.order_loc.get(&order_id) {
            let current_deque = match side {
                Side::Bid => self.bid_book.price_levels.get_mut(*price_level).unwrap(),
                Side::Ask => self.ask_book.price_levels.get_mut(*price_level).unwrap(),
            };
            current_deque.retain(|x| x.0 != order_id);
            self.order_loc.remove(&order_id);
            Ok("Successfully cancelled order")
        } else {
            Err("No such order id")
        }
    }

    fn create_new_limit_order(&mut self, order: JsonOrder) -> Hash {
        let encoded_order = order.encode_order();
        let order_id = encoded_order.hash_hex();
        let book = match order.side {
            Side::Ask => &mut self.ask_book,
            Side::Bid => &mut self.bid_book,
        };

        if let Some(val) = book.price_map.get(&encoded_order.price) {
            book.price_levels[*val].push_back((order_id.clone(), encoded_order));
            self.order_loc.insert(order_id.clone(), (order.side, *val));
        } else {
            let new_loc = book.price_levels.len();
            book.price_map.insert(encoded_order.price, new_loc);
            let mut vec_deq = VecDeque::new();
            vec_deq.push_back((order_id.clone(), encoded_order));
            book.price_levels.push(vec_deq);
            self.order_loc
                .insert(order_id.clone(), (order.side, new_loc));
        }
        order_id
    }

    fn match_at_price_level(
        fill_result: &mut FillResult,
        price_level: &mut VecDeque<(Hash, Order)>,
        order_loc: &mut HashMap<Hash, (Side, usize)>,
        maker_order: &Hash,
    ) {
        for (order_id, order) in price_level.iter_mut() {
            let fill: Fill;
            if order.amount <= fill_result.remaining {
                fill = Fill {
                    maker_hash: maker_order.clone(),
                    taker_hash: order_id.clone(),
                    fill_amount: order.amount.clone(),
                    price: order.price.clone(),
                };
                fill_result.remaining -= order.amount;
                order.amount = U256::zero();
                order_loc.remove(order_id);
            } else {
                fill = Fill {
                    maker_hash: maker_order.clone(),
                    taker_hash: order_id.clone(),
                    fill_amount: fill_result.remaining.clone(),
                    price: order.price.clone(),
                };
                order.amount -= fill_result.remaining;
                fill_result.remaining = U256::zero();
            }
            fill_result.filled_orders.push(fill);
            if fill_result.remaining <= U256::from(ERROR) {
                // order is all filled.
                break;
            }
        }
        // remove filled orders from the order book.
        price_level.retain(|x| x.1.amount > U256::from(ERROR));
    }

    pub fn add_limit_order(&mut self, order: JsonOrder) -> FillResult {
        let encoded_order = order.encode_order();
        let maker_order = encoded_order.hash_hex();
        debug!(
            "Got order with amount {}, at price {}",
            order.amount, order.price
        );
        let mut fill_result = FillResult::new(encoded_order.amount, order.side.clone());
        match order.side {
            Side::Bid => {
                let ask_book = &mut self.ask_book;
                let price_map = &mut ask_book.price_map;
                let price_levels = &mut ask_book.price_levels;
                let mut price_map_iter = price_map.iter();

                if let Some((mut x, _)) = price_map_iter.next() {
                    while &encoded_order.price >= x {
                        let curr_level = price_map[x];
                        Self::match_at_price_level(
                            &mut fill_result,
                            &mut price_levels[curr_level],
                            &mut self.order_loc,
                            &maker_order,
                        );
                        if let Some((a, _)) = price_map_iter.next() {
                            x = a;
                        } else {
                            break;
                        }
                    }
                }
            }
            Side::Ask => {
                let bid_book = &mut self.bid_book;
                let price_map = &mut bid_book.price_map;
                let price_levels = &mut bid_book.price_levels;
                let mut price_map_iter = price_map.iter();

                if let Some((mut x, _)) = price_map_iter.next_back() {
                    while &encoded_order.price <= x {
                        let curr_level = price_map[x];
                        Self::match_at_price_level(
                            &mut fill_result,
                            &mut price_levels[curr_level],
                            &mut self.order_loc,
                            &maker_order,
                        );
                        if let Some((a, _)) = price_map_iter.next_back() {
                            x = a;
                        } else {
                            break;
                        }
                    }
                }
            }
        }
        if fill_result.remaining > U256::from(ERROR) {
            let remaining_decimal = u256_to_decimal(&fill_result.remaining);
            debug!(
                "Still remaining amount {} at price level {}",
                remaining_decimal, order.price
            );
            fill_result.status = OrderStatus::PartiallyFilled;
            let mut new_order = order.clone();
            new_order.amount = remaining_decimal;
            self.create_new_limit_order(new_order);
        } else {
            fill_result.status = OrderStatus::Filled;
        }
        fill_result
    }

    pub fn generate_l2_order_book(&self) -> L2OrderBook {
        let mut l2 = L2OrderBook::new();
        let mut ask_price_map_iter = self.ask_book.price_map.iter();
        let mut count = L2_MAX;
        // get lowest ask prices.
        while count > 0 {
            if let Some((x, _)) = ask_price_map_iter.next() {
                let curr_level = self.ask_book.price_map[x];
                let price_level = &self.ask_book.price_levels[curr_level];
                for (_, order) in price_level {
                    let simple = SimpleOrder {
                        amount: u256_to_decimal(&order.amount),
                        price: u256_to_decimal(&order.price),
                    };
                    l2.asks.push(simple);
                    count -= 1;
                    if count == 0 {
                        break;
                    }
                }
            }
        }
        let mut bid_price_map_iter = self.bid_book.price_map.iter();
        count = L2_MAX;
        // get highest bid price.
        while count > 0 {
            if let Some((x, _)) = bid_price_map_iter.next_back() {
                let curr_level = self.bid_book.price_map[x];
                let price_level = &self.bid_book.price_levels[curr_level];
                for (_, order) in price_level {
                    let simple = SimpleOrder {
                        amount: u256_to_decimal(&order.amount),
                        price: u256_to_decimal(&order.price),
                    };
                    l2.bids.push(simple);
                    count -= 1;
                    if count == 0 {
                        break;
                    }
                }
            }
        }
        l2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex;
    use num_bigint::{BigUint, RandomBits};
    use rand::{rngs::StdRng, Rng, SeedableRng};

    #[test]
    fn eip_712() {
        let order = Order {
            amount: U256::from(1234),
            nonce: U256::from(12),
            price: U256::from(5432),
            side: 0 as u8,
            traderAddress: "0x3A880652F47bFaa771908C07Dd8673A787dAEd3A"
                .parse::<Address>()
                .expect("Failed to parse trader's address!"),
        };
        let hash_str = order.hash_hex();
        assert_eq!(
            "0x15a7b83cc86b50aaa2fa0c0871d5dbaae62f116436291e976c84b034b58cb728",
            hash_str
        );
    }

    fn get_nonce(seed: u64) -> String {
        let mut rng: StdRng = SeedableRng::seed_from_u64(seed);
        let nonce_bits: BigUint = rng.sample(RandomBits::new(256));
        hex::encode(nonce_bits.to_bytes_le())
    }

    #[test]
    fn json_order() {
        let nonce_hex = get_nonce(9998);
        let json_order = JsonOrder {
            amount: "1.0".to_string(),
            nonce: nonce_hex,
            price: "1.3".to_string(),
            side: Side::Bid,
            traderAddress: "0x3A880652F47bFaa771908C07Dd8673A787dAEd3A"
                .parse::<Address>()
                .expect("Failed to parse trader's address!"),
        };
        let order = json_order.encode_order();
        let hash_str = order.hash_hex();
        assert_eq!(
            "0x768858949ae9d5453e35736fa634f5d0f46d2ab00880551bc3533169239e022e",
            hash_str
        );
    }

    #[test]
    fn order_book_case_1() {
        let alice_address = "0xb794f5ea0ba39494ce839613fffba74279579268"
            .parse::<Address>()
            .expect("Failed to parse trader's address!");
        let alice_json = JsonAccount {
            ddxBalance: "0.0".to_string(),
            usdBalance: "10.0".to_string(),
            traderAddress: alice_address.clone(),
        };
        let mut alice = Account::from_json(alice_json);
        let bob_address = "0x3A880652F47bFaa771908C07Dd8673A787dAEd3A"
            .parse::<Address>()
            .expect("Failed to parse trader's address!");
        let bob_json = JsonAccount {
            ddxBalance: "1.0".to_string(),
            usdBalance: "0.0".to_string(),
            traderAddress: bob_address.clone(),
        };
        let mut bob = Account::from_json(bob_json);
        let mut order_book = OrderBook::new("DDX".to_string());
        let alice_order = JsonOrder {
            amount: "1.0".to_string(),
            price: "10.0".to_string(),
            side: Side::Bid,
            nonce: get_nonce(1),
            traderAddress: alice_address.clone(),
        };
        order_book.add_limit_order(alice_order);
        let bob_order = JsonOrder {
            amount: "1.0".to_string(),
            price: "8.0".to_string(),
            side: Side::Ask,
            nonce: get_nonce(2),
            traderAddress: bob_address.clone(),
        };
        let fill_result = order_book.add_limit_order(bob_order);
        bob.update(
            fill_result.side.clone(),
            fill_result.filled_orders[0].clone(),
        );
        assert_eq!(order_book.order_loc.len(), 0);
    }
}
