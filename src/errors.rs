use alloy_primitives::{Address, U256};
use revm::primitives::B256;
use thiserror::Error;

/// Wrapper for Database errors
#[derive(Error, Debug)]
pub enum DatabaseError {
    #[error("missing AccountInfo {0}")]
    MissingAccount(Address),
    #[error("code should already be loaded: {0}")]
    MissingCode(B256),
    #[error("failed to get account for {0}")]
    GetAccount(Address),
    #[error("failed to get storage for {0} at {1}")]
    GetStorage(Address, U256),
    #[error("failed to get block hash for {0}")]
    GetBlockHash(U256),
}
