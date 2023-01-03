//! In-memory account management.
use crate::fill::Fill;
use crate::json::JsonOrder;
use crate::json::{JsonAccount, Side};
use crate::order::Order;
use crate::primitive::{Address, decimal_to_u256, u256_to_decimal};
use crate::FillResult;
use ethers::types::U256;
use std::collections::HashMap;
use std::ops::Div;

// max account balance error.
pub const ERROR: u16 = 10000;

#[derive(Debug, Clone)]
pub struct Account {
    _username: String,
    ddx_balance: U256,
    ddx_hold: U256,
    usd_balance: U256,
    usd_hold: U256,
    trader_address: Address,
}

impl Account {
    pub fn from_json(user: String, json: JsonAccount) -> Self {
        Self {
            _username: user,
            ddx_balance: decimal_to_u256(&json.ddxBalance),
            ddx_hold: U256::zero(),
            usd_balance: decimal_to_u256(&json.usdBalance),
            usd_hold: U256::zero(),
            trader_address: json.traderAddress.clone(),
        }
    }

    pub fn to_json(&self) -> JsonAccount {
        JsonAccount {
            ddxBalance: u256_to_decimal(&self.total_ddx()),
            usdBalance: u256_to_decimal(&self.total_usd()),
            traderAddress: self.trader_address.clone(),
        }
    }

    pub fn update(&mut self, side: Side, fill: &Fill) {
        let unit_scale = U256::from(1e18 as u64);
        match side {
            Side::Bid => {
                assert_eq!(
                    self.trader_address, fill.to,
                    "Filled bid order contains mismatched data!"
                );
                self.ddx_balance += fill.fill_amount;
                self.usd_hold -= fill.fill_amount.saturating_mul(fill.price).div(unit_scale);
            }
            Side::Ask => {
                assert_eq!(
                    self.trader_address, fill.from,
                    "Filled ask order contains mismatched data!"
                );
                self.ddx_hold -= fill.fill_amount;
                self.usd_balance += fill.fill_amount.saturating_mul(fill.price).div(unit_scale);
            }
        }
    }

    pub fn total_ddx(&self) -> U256 {
        self.ddx_balance + self.ddx_hold
    }

    pub fn total_usd(&self) -> U256 {
        self.usd_balance + self.usd_hold
    }
}

#[derive(Debug)]
pub struct AccountManager {
    accounts: HashMap<Address, Account>,
}

impl AccountManager {
    pub fn new() -> Self {
        AccountManager {
            accounts: HashMap::new(),
        }
    }
    pub fn new_account(&mut self, user: &str, address: Address) {
        let account = Account {
            _username: user.to_string(),
            ddx_balance: U256::zero(),
            ddx_hold: U256::zero(),
            usd_balance: U256::zero(),
            usd_hold: U256::zero(),
            trader_address: address,
        };
        self.accounts.insert(address, account);
    }

    pub fn add_json_account(&mut self, user: &str, json: JsonAccount) {
        let address = json.traderAddress.clone();
        let account = Account::from_json(user.to_string(), json);
        self.accounts.insert(address, account);
    }

    pub fn delete_account(&mut self, address: &Address) -> Option<JsonAccount> {
        if let Some(account) = self.accounts.remove(address) {
            Some(account.to_json())
        } else {
            None
        }
    }

    pub fn get_json_account(&self, address: &Address) -> Option<JsonAccount> {
        if let Some(account) = self.accounts.get(address) {
            Some(account.to_json())
        } else {
            None
        }
    }

    /// Generate a validate order from available account balance.
    pub fn validate_order(&mut self, order: JsonOrder) -> Option<Order> {
        if let Some(account) = self.accounts.get_mut(&order.traderAddress) {
            let unit_scale = U256::from(1e18 as u64);
            let encoded_order = order.encode_order();
            match order.side {
                Side::Bid => {
                    let diff = encoded_order
                        .amount
                        .saturating_mul(encoded_order.price)
                        .div(unit_scale);
                    if diff <= U256::from(ERROR) + account.usd_balance {
                        account.usd_balance -= diff;
                        account.usd_hold += diff;
                    } else {
                        return None;
                    }
                }
                Side::Ask => {
                    if encoded_order.amount <= U256::from(ERROR) + account.ddx_balance {
                        account.ddx_balance -= encoded_order.amount;
                        account.ddx_hold += encoded_order.amount;
                    } else {
                        return None;
                    }
                }
            }
            Some(encoded_order)
        } else {
            None
        }
    }

    /// Revert pending balance from canceled order and make it available to new orders.
    pub fn release_pending_fund(&mut self, cancelled_order: &Order) -> Option<Account> {
        if let Some(account) = self.accounts.get_mut(&cancelled_order.traderAddress) {
            let unit_scale = U256::from(1e18 as u64);
            match cancelled_order.get_side() {
                Side::Bid => {
                    let diff = cancelled_order
                        .amount
                        .saturating_mul(cancelled_order.price)
                        .div(unit_scale);
                    assert!(
                        diff <= U256::from(ERROR) + account.usd_hold,
                        "User account pending USD balance mismatch!"
                    );
                    account.usd_balance += diff;
                    account.usd_hold -= diff;
                }
                Side::Ask => {
                    assert!(
                        cancelled_order.amount <= U256::from(ERROR) + account.ddx_hold,
                        "User account pending DDX balance mismatch!"
                    );
                    account.ddx_balance += cancelled_order.amount;
                    account.ddx_hold -= cancelled_order.amount;
                }
            }
            Some(account.clone())
        } else {
            None
        }
    }

    pub fn update_accounts(&mut self, fill_result: FillResult) {
        for fill in fill_result.filled_orders {
            if self.accounts.contains_key(&fill.from) {
                let account = self.accounts.get_mut(&fill.from).unwrap();
                account.update(Side::Ask, &fill);
            }
            if self.accounts.contains_key(&fill.to) {
                let account = self.accounts.get_mut(&fill.to).unwrap();
                account.update(Side::Bid, &fill);
            }
        }
    }
}
