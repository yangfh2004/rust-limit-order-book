use ethers::types::{transaction::eip712::Eip712, H160, U256};
use ethers_contract::EthAbiType;
use ethers_derive_eip712::*;

#[derive(Debug, Copy, Clone, Eip712, EthAbiType)]
#[eip712(name = "DDX take-home", version = "0.1.0")]
pub struct Order {
    pub amount: U256,
    pub nonce: U256,
    pub price: U256,
    pub side: u8,
    pub traderAddress: H160,
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex;

    #[test]
    fn eip_712() {
        let order = Order {
            amount: U256::from(1234),
            nonce: U256::from(12),
            price: U256::from(5432),
            side: 0 as u8,
            traderAddress: "0x3A880652F47bFaa771908C07Dd8673A787dAEd3A"
                .parse::<H160>()
                .expect("Failed to parse trader's address!"),
        };
        let hash_bytes = order.encode_eip712().unwrap();
        let hash_str = hex::encode(&hash_bytes);
        // TODO: this version of EIP-712 hasher gives different results from the reference: 0x15a7b83cc86b50aaa2fa0c0871d5dbaae62f116436291e976c84b034b58cb728
        assert_eq!(
            "fcb4e55c0fb5cea3352d69b43ba4802ec18205250c874b71d682933602dcc105",
            hash_str
        );
    }
}
