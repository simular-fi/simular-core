//! `simular-core` is a library for interacting with an embedded Ethereum Virtual Machine.
//! It provides the following core modules:
//! - `abi`          : parsing ABI files, encoding/decoding calls to smart contracts
//! - `forkdb/memdb` : backend storage for the EVM
//! - `baseevm`      : manipulate accounts, send transactions
pub mod abi;
pub mod baseevm;
pub mod errors;
pub mod forkdb;
pub mod memdb;
pub mod snapshot;

// re-exports
pub use {abi::ContractAbi, baseevm::EvmFork, baseevm::EvmMemory, snapshot::SerializableState};
