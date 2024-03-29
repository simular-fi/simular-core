use revm::primitives::{Address, Bytes, U256};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Source of the snapshop.  Either from a fork or the local in-memory DB.
#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub enum SerializingSource {
    Memory,
    #[default]
    Fork,
}

/// A single AccountRecord and it's associated storage
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SerializableAccountRecord {
    pub nonce: u64,
    pub balance: U256,
    pub code: Bytes,
    pub storage: BTreeMap<U256, U256>,
}

/// The high-level objects containing the snapshot.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SerializableState {
    pub source: SerializingSource,
    pub block_num: u64,
    pub accounts: BTreeMap<Address, SerializableAccountRecord>,
}
