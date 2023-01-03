//! A limit-order match engine supports ETH account address and EIP-712 hashing signature
//! with a l2 order-book generator.
pub mod account;
pub mod fill;
pub mod json;
pub mod order;
pub mod primitive;

use crate::account::{AccountManager, ERROR};
use crate::fill::{Fill, FillResult};
use crate::json::{JsonOrder, L2OrderBook, Side, SimpleOrder};
use crate::order::Order;
use crate::primitive::{Address, Hash, OrderStatus, u256_to_decimal};
use ethers::types::U256;
use log::debug;
use std::collections::{BTreeMap, HashMap};
// constants
const ORDER_BOOK_INIT_CAP: usize = 50_000;
const L2_MAX: usize = 50;

#[derive(Debug)]
struct HalfBook {
    _side: Side,
    price_map: BTreeMap<U256, usize>,
    price_levels: Vec<HashMap<Hash, Order>>,
}

impl HalfBook {
    pub fn new(side: Side) -> Self {
        HalfBook {
            _side: side,
            price_map: BTreeMap::new(),
            price_levels: Vec::with_capacity(ORDER_BOOK_INIT_CAP),
        }
    }
}

#[derive(Debug)]
pub struct OrderBook {
    _symbol: String,
    bid_book: HalfBook,
    ask_book: HalfBook,
    // For fast cancels Order Hash -> (Side, Price_level)
    order_loc: HashMap<Hash, (Side, usize)>,
}

impl OrderBook {
    pub fn new(symbol: String) -> Self {
        OrderBook {
            _symbol: symbol,
            bid_book: HalfBook::new(Side::Bid),
            ask_book: HalfBook::new(Side::Ask),
            order_loc: HashMap::with_capacity(ORDER_BOOK_INIT_CAP),
        }
    }

    pub fn get_order(&self, order_id: Hash) -> Result<JsonOrder, &str> {
        if let Some((side, price_level)) = self.order_loc.get(&order_id) {
            let current_map = match side {
                Side::Bid => self.bid_book.price_levels.get(*price_level).unwrap(),
                Side::Ask => self.ask_book.price_levels.get(*price_level).unwrap(),
            };
            let order = current_map.get(&order_id).unwrap();
            Ok(order.to_json())
        } else {
            Err("No such order id")
        }
    }

    pub fn cancel_order(
        &mut self,
        manager: &mut AccountManager,
        order_id: Hash,
    ) -> Result<JsonOrder, &str> {
        if let Some((side, price_level)) = self.order_loc.get(&order_id) {
            let current_map = match side {
                Side::Bid => self.bid_book.price_levels.get_mut(*price_level).unwrap(),
                Side::Ask => self.ask_book.price_levels.get_mut(*price_level).unwrap(),
            };
            let order = current_map.remove(&order_id).unwrap();
            self.order_loc.remove(&order_id);
            // restore user's account balance after cancellation.
            manager.release_pending_fund(&order);
            Ok(order.to_json())
        } else {
            Err("No such order id")
        }
    }

    fn create_new_limit_order(&mut self, side: Side, order: Order) -> Hash {
        let order_id = order.hash_hex();
        let book = match side {
            Side::Ask => &mut self.ask_book,
            Side::Bid => &mut self.bid_book,
        };

        if let Some(val) = book.price_map.get(&order.price) {
            book.price_levels[*val].insert(order_id.clone(), order);
            self.order_loc.insert(order_id.clone(), (side, *val));
        } else {
            let new_loc = book.price_levels.len();
            book.price_map.insert(order.price, new_loc);
            let mut new_map = HashMap::new();
            new_map.insert(order_id.clone(), order);
            book.price_levels.push(new_map);
            self.order_loc.insert(order_id.clone(), (side, new_loc));
        }
        order_id
    }

    fn match_at_price_level(
        fill_result: &mut FillResult,
        price_level: &mut HashMap<Hash, Order>,
        order_loc: &mut HashMap<Hash, (Side, usize)>,
        maker_order: &Hash,
        trader_addr: &Address,
        side: Side,
    ) {
        for (order_id, order) in price_level.iter_mut() {
            let fill: Fill;
            let (from, to) = match side {
                Side::Bid => (order.traderAddress, trader_addr.clone()),
                Side::Ask => (trader_addr.clone(), order.traderAddress),
            };
            // self-match prevention.
            if from != to {
                if order.amount <= fill_result.remaining {
                    fill = Fill {
                        from,
                        to,
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
                        from,
                        to,
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
        }
        // remove filled orders from the order book.
        price_level.retain(|_, o| o.amount > U256::from(ERROR));
    }

    pub fn add_order(
        &mut self,
        manager: &mut AccountManager,
        order: JsonOrder,
    ) -> Option<FillResult> {
        if let Some(encoded_order) = manager.validate_order(order.clone()) {
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
                                &order.traderAddress,
                                Side::Bid,
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
                                &order.traderAddress,
                                Side::Ask,
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
                let mut new_order = encoded_order.clone();
                new_order.amount = fill_result.remaining;
                self.create_new_limit_order(order.side, new_order);
            } else {
                fill_result.status = OrderStatus::Filled;
            }
            Some(fill_result)
        } else {
            None
        }
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
                    if count <= 0 {
                        break;
                    }
                }
            } else {
                break;
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
                    if count <= 0 {
                        break;
                    }
                }
            } else {
                break;
            }
        }
        l2
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::JsonAccount;
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
        format!("0x{}", hex::encode(nonce_bits.to_bytes_le()))
    }

    fn order_init(seed: u64) -> JsonOrder {
        let nonce_hex = get_nonce(seed);
        JsonOrder {
            amount: "1.0".to_string(),
            nonce: nonce_hex,
            price: "10.0".to_string(),
            side: Side::Bid,
            traderAddress: "0xb794f5ea0ba39494ce839613fffba74279579268"
                .parse::<Address>()
                .expect("Failed to parse trader's address!"),
        }
    }

    #[test]
    fn json_order() {
        let json_order = order_init(9998);
        debug!("{:}", json_order);
        let order = json_order.encode_order();
        let hash_str = order.hash_hex();
        assert_eq!(
            "0x47f84837be59a0e7c6f9bc9af3c3e80971d8a589002dea75732137fe17ec3e1e",
            hash_str
        );
    }

    #[test]
    fn get_order() {
        let (alice_address, bob_address) = address_init();
        let mut manager = account_init(&alice_address, "0.0", "10.0", &bob_address, "1.0", "0.0");
        let mut order_book = OrderBook::new("DDX".to_string());
        let alice_order = JsonOrder {
            amount: "1.0".to_string(),
            price: "10.0".to_string(),
            side: Side::Bid,
            nonce: get_nonce(1),
            traderAddress: alice_address.clone(),
        };
        order_book
            .add_order(&mut manager, alice_order.clone())
            .unwrap();
        let hash_str = alice_order.encode_order().hash_hex();
        let order = order_book.get_order(hash_str);
        assert!(order.is_ok(), "Cannot get order with EIP712 hash!");
    }

    #[test]
    fn cancel_order() {
        let (alice_address, bob_address) = address_init();
        let mut manager = account_init(&alice_address, "0.0", "10.0", &bob_address, "1.0", "0.0");
        let mut order_book = OrderBook::new("DDX".to_string());
        let alice_order = JsonOrder {
            amount: "1.0".to_string(),
            price: "10.0".to_string(),
            side: Side::Bid,
            nonce: get_nonce(1),
            traderAddress: alice_address.clone(),
        };
        order_book
            .add_order(&mut manager, alice_order.clone())
            .unwrap();
        let hash_str = alice_order.encode_order().hash_hex();
        let order = order_book.cancel_order(&mut manager, hash_str);
        assert!(order.is_ok(), "Cannot get order with EIP712 hash!");
    }

    fn account_init(
        alice_addr: &Address,
        alice_ddx: &str,
        alice_usd: &str,
        bob_addr: &Address,
        bob_ddx: &str,
        bob_usd: &str,
    ) -> AccountManager {
        let mut manager = AccountManager::new();
        let alice_json = JsonAccount {
            ddxBalance: alice_ddx.to_string(),
            usdBalance: alice_usd.to_string(),
            traderAddress: alice_addr.clone(),
        };
        manager.add_json_account("alice", alice_json);
        let bob_json = JsonAccount {
            ddxBalance: bob_ddx.to_string(),
            usdBalance: bob_usd.to_string(),
            traderAddress: bob_addr.clone(),
        };
        manager.add_json_account("bob", bob_json);
        manager
    }

    fn address_init() -> (Address, Address) {
        let alice_address = "0xb794f5ea0ba39494ce839613fffba74279579268"
            .parse::<Address>()
            .expect("Failed to parse trader's address!");
        let bob_address = "0x3A880652F47bFaa771908C07Dd8673A787dAEd3A"
            .parse::<Address>()
            .expect("Failed to parse trader's address!");
        (alice_address, bob_address)
    }

    #[test]
    fn order_book_case_1() {
        let (alice_address, bob_address) = address_init();
        let mut manager = account_init(&alice_address, "0.0", "10.0", &bob_address, "1.0", "0.0");
        let mut order_book = OrderBook::new("DDX".to_string());
        let alice_order = JsonOrder {
            amount: "1.0".to_string(),
            price: "10.0".to_string(),
            side: Side::Bid,
            nonce: get_nonce(1),
            traderAddress: alice_address.clone(),
        };
        let fill_result = order_book.add_order(&mut manager, alice_order).unwrap();
        manager.update_accounts(fill_result);
        let bob_order = JsonOrder {
            amount: "1.0".to_string(),
            price: "8.0".to_string(),
            side: Side::Ask,
            nonce: get_nonce(2),
            traderAddress: bob_address.clone(),
        };
        let fill_result = order_book.add_order(&mut manager, bob_order).unwrap();
        manager.update_accounts(fill_result);
        // check if order book is empty.
        assert_eq!(order_book.order_loc.len(), 0);
        // check the balance of alice and bob.
        assert_eq!(
            manager.get_json_account(&alice_address).unwrap().ddxBalance,
            "1.00"
        );
        assert_eq!(
            manager.get_json_account(&bob_address).unwrap().usdBalance,
            "10.00"
        );
    }

    #[test]
    fn order_book_test_2() {
        let (alice_address, bob_address) = address_init();
        let mut manager = account_init(&alice_address, "0.0", "10.0", &bob_address, "1.0", "0.0");
        let mut order_book = OrderBook::new("DDX".to_string());
        let bob_order = JsonOrder {
            amount: "1.0".to_string(),
            price: "10.0".to_string(),
            side: Side::Ask,
            nonce: get_nonce(1),
            traderAddress: bob_address.clone(),
        };
        let fill_result = order_book.add_order(&mut manager, bob_order).unwrap();
        manager.update_accounts(fill_result);
        let alice_order = JsonOrder {
            amount: "0.5".to_string(),
            price: "12.0".to_string(),
            side: Side::Bid,
            nonce: get_nonce(2),
            traderAddress: alice_address.clone(),
        };
        let fill_result = order_book.add_order(&mut manager, alice_order).unwrap();
        manager.update_accounts(fill_result);
        // check if order book has a partially filled order.
        assert_eq!(order_book.order_loc.len(), 1);
        // check the balance of alice and bob.
        let alice_json = manager.get_json_account(&alice_address).unwrap();
        assert_eq!(alice_json.ddxBalance, "0.50");
        assert_eq!(alice_json.usdBalance, "5.00");
        let bob_json = manager.get_json_account(&bob_address).unwrap();
        assert_eq!(bob_json.ddxBalance, "0.50");
        assert_eq!(bob_json.usdBalance, "5.00");
    }

    #[test]
    fn order_book_test_3() {
        let (alice_address, bob_address) = address_init();
        let mut manager = account_init(&alice_address, "0.0", "10.0", &bob_address, "3.0", "10.0");
        let mut order_book = OrderBook::new("DDX".to_string());
        let alice_order = JsonOrder {
            amount: "1.0".to_string(),
            price: "10.0".to_string(),
            side: Side::Bid,
            nonce: get_nonce(1),
            traderAddress: alice_address.clone(),
        };
        let fill_result = order_book.add_order(&mut manager, alice_order).unwrap();
        manager.update_accounts(fill_result);
        let bob_order = JsonOrder {
            amount: "1.0".to_string(),
            price: "10.0".to_string(),
            side: Side::Bid,
            nonce: get_nonce(2),
            traderAddress: bob_address.clone(),
        };
        let fill_result = order_book.add_order(&mut manager, bob_order).unwrap();
        manager.update_accounts(fill_result);
        let bob_order = JsonOrder {
            amount: "1.0".to_string(),
            price: "11.0".to_string(),
            side: Side::Ask,
            nonce: get_nonce(3),
            traderAddress: bob_address.clone(),
        };
        let fill_result = order_book.add_order(&mut manager, bob_order).unwrap();
        manager.update_accounts(fill_result);
        let bob_order = JsonOrder {
            amount: "2.0".to_string(),
            price: "9.0".to_string(),
            side: Side::Ask,
            nonce: get_nonce(4),
            traderAddress: bob_address.clone(),
        };
        let fill_result = order_book.add_order(&mut manager, bob_order).unwrap();
        manager.update_accounts(fill_result);
        // check if order book has a partially filled order.
        assert_eq!(order_book.order_loc.len(), 3);
        // check the balance of alice and bob.
        let alice_json = manager.get_json_account(&alice_address).unwrap();
        assert_eq!(alice_json.ddxBalance, "1.00");
        assert_eq!(alice_json.usdBalance, "0.00");
        let bob_json = manager.get_json_account(&bob_address).unwrap();
        assert_eq!(bob_json.ddxBalance, "2.00");
        assert_eq!(bob_json.usdBalance, "20.00");
    }

    #[test]
    fn invalidate_order() {
        let (alice_address, bob_address) = address_init();
        let mut manager = account_init(&alice_address, "0.0", "10.0", &bob_address, "1.0", "0.0");
        let mut order_book = OrderBook::new("DDX".to_string());
        let alice_order = JsonOrder {
            amount: "2.0".to_string(),
            price: "10.0".to_string(),
            side: Side::Bid,
            nonce: get_nonce(1),
            traderAddress: alice_address.clone(),
        };
        let fill_result = order_book.add_order(&mut manager, alice_order);
        assert!(
            fill_result.is_none(),
            "The trader makes bids more than its available liquidation"
        );
        let bob_order = JsonOrder {
            amount: "2.0".to_string(),
            price: "8.0".to_string(),
            side: Side::Ask,
            nonce: get_nonce(2),
            traderAddress: bob_address.clone(),
        };
        let fill_result = order_book.add_order(&mut manager, bob_order);
        assert!(
            fill_result.is_none(),
            "The trader makes asks more than its available liquidation"
        );
    }

    #[test]
    fn generate_l2_book() {
        let mut order_book = OrderBook::new("DDX".to_string());
        let mut rng = rand::thread_rng();
        let (alice_address, bob_address) = address_init();
        let mut manager = account_init(
            &alice_address,
            "1000.0",
            "1000.0",
            &bob_address,
            "1000.0",
            "2000.0",
        );
        for _ in 0..100 {
            let (alice_address, bob_address) = address_init();
            let alice_order = JsonOrder {
                amount: format!("{:.2}", rng.gen_range(0.0..10.0)),
                price: format!("{:.2}", rng.gen_range(0.0..10.0)),
                side: Side::Bid,
                nonce: get_nonce(1),
                traderAddress: alice_address.clone(),
            };
            order_book.add_order(&mut manager, alice_order);
            let bob_order = JsonOrder {
                amount: format!("{:.2}", rng.gen_range(0.0..10.0)),
                price: format!("{:.2}", rng.gen_range(10.0..20.0)),
                side: Side::Ask,
                nonce: get_nonce(2),
                traderAddress: bob_address.clone(),
            };
            order_book.add_order(&mut manager, bob_order);
        }
        let l2_book = order_book.generate_l2_order_book();
        assert!(l2_book.asks.len() <= 50);
        assert!(l2_book.bids.len() <= 50);
    }
}
