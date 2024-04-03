//! This library provides a simple API for interating with an embedded Ethereum Virtual Machine (EVM).
//!
//! <br/>
//!
//! ```toml
//! [dependencies]
//! simular-core = "0.2.2"
//! ```
//!
//! # Supports
//! - Both an in-memory database and the ability to fork state from a remote node
//! - Snapshot of EVM state for later use
//! - Both raw encoding/decoding of function calls as well as [alloy SolTypes](https://docs.rs/alloy-sol-macro/0.7.0/alloy_sol_macro/macro.sol.html)
//! - Lightweight creation of ABI from [human-readable](https://docs.ethers.org/v5/api/utils/abi/formats/#abi-formats--human-readable-abi) syntax
//!
//!
//! # Examples
//!
//! - Create and interact with the EVM using the the in-memory database.
//!
//!   ```
//!     use simular_core::{BaseEvm, generate_random_addresses};
//!     use alloy_primitives::{Address, U256};
//!
//!     // Generate some random addresses
//!     let addresses = generate_random_addresses(2);
//!     let bob = addresses[0];
//!     let alice = addresses[1];
//!
//!     // create the EVM with in-memory database (default)
//!     let mut evm = BaseEvm::default();
//!     
//!     // create 2 accounts. Bob w/ 2 ether, alice w/ none
//!     evm.create_account(bob, Some(U256::from(2e18))).unwrap();
//!     evm.create_account(alice, None).unwrap();
//!
//!     // check the balances
//!     assert!(evm.get_balance(bob).unwrap() == U256::from(2e18));
//!     assert!(evm.get_balance(alice).unwrap() == U256::from(0));
//!   ```
//!
//! - Fork a remote contract.  Interacting with a remote contract pulls the state of the remote contract
//!   into the local in-memory database for use.
//!
//!   ```
//!     use simular_core::{BaseEvm, generate_random_addresses, ContractAbi;
//!     use alloy_primitives::{Address, U256, address};
//!     
//!     let abi = ContractAbi::from_human_readable(vec![
//!     "function totalSupply() (uint256)"
//!     ]);
//!    
//!     
//!     // create a fork using the latest block
//!     let fork_info = CreateFork.latest_block(URL OF JSON-RPC NODE);
//!     let mut evm = BaseEvm::new(Some(fork_info));
//!
//!     // remote contract address.
//!     // using DAI: 0x6B175474E89094C44Da98b954EedeAC495271d0F
//!     let contract_address = address!("6B175474E89094C44Da98b954EedeAC495271d0F");
//!     
//!     // encode the function call
//!     let (encoded_total_supply, _, decoder) =
//!         abi.encode_function("totalSupply", "()").unwrap();
//!
//!     // call the function on the remote contract
//!     let output = evm.transact_call(
//!         contract_address,
//!         encoded_total_supply,
//!         U256::from(0)).unwrap();
//!
//!     // decode the result
//!     let value = decoder.abi_decode(&output.result)
//!     
//!     println!("total supply: {:?}", value);
//!   ```
//!
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
