use ethers_contract::EthAbiType;
use ethers_derive_eip712::*;
use ethers::types::{transaction::eip712::Eip712, H160 as Address, U256};
use hex;

#[derive(Debug, Copy, Clone, Eip712, EthAbiType)]
#[eip712(
    name = "DDX take-home",
    version = "0.1.0",
)]
#[allow(non_snake_case)]
pub struct Order {
    pub amount: U256,
    pub nonce: U256,
    pub price: U256,
    pub side: u8,
    pub traderAddress: Address,
}

impl Order {
    pub fn hash_hex(&self) -> String {
        let hash_bytes = self.encode_eip712().unwrap();
        let mut prefix = "0x".to_string();
        let hash_str = hex::encode(&hash_bytes);
        prefix.push_str(&hash_str);
        prefix
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eip_712() {
        let order = Order{
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
}
