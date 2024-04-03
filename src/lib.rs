//! `simular-core` is a library for interacting with an embedded Ethereum Virtual Machine.
//! It provides the following core modules:
//! - `abi`          : parsing ABI files, encoding/decoding calls to smart contracts
//! - `forkdb/memdb` : backend storage for the EVM
//! - `baseevm`      : manipulate accounts, send transactions
pub mod abi;
pub mod db;
pub mod errors;
pub mod evm;
pub mod snapshot;

// re-exports
pub use {abi::ContractAbi, db::CreateFork, evm::BaseEvm, snapshot::SnapShot};

use alloy_primitives::Address;

/// Generate the given `num` of addresses
pub fn generate_random_addresses(num: u8) -> Vec<Address> {
    let mut addresses: Vec<alloy_primitives::Address> = Vec::new();
    for i in 1..=num {
        addresses.push(Address::repeat_byte(i));
    }
    addresses
}
