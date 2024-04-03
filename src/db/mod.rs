pub(crate) mod fork;
pub(crate) mod fork_backend;
pub(crate) mod in_memory_db;

use crate::{
    db::{fork::Fork, in_memory_db::SimularEvmInMemoryDB},
    errors::DatabaseError,
    snapshot::{SnapShot, SnapShotAccountRecord, SnapShotSource},
};

use alloy_primitives::{Address, U256};
use anyhow::{anyhow, Result};
use revm::{
    interpreter::primitives::EnvWithHandlerCfg,
    primitives::{
        Account, AccountInfo, Bytecode, HashMap as Map, ResultAndState, B256, KECCAK_EMPTY,
    },
    Database, DatabaseCommit, DatabaseRef, EvmBuilder,
};

/// Information related to creating a fork
#[derive(Clone, Debug)]
pub struct CreateFork {
    /// the url of the RPC endpoint
    pub url: String,
    /// optional block number of the fork.  If none, it will use the latest block.
    pub blocknumber: Option<u64>,
}

impl CreateFork {
    /// Fork at the given URL and block number
    pub fn new(url: String, blocknumber: Option<u64>) -> Self {
        Self { url, blocknumber }
    }

    /// For at the given URL and use the latest block available
    pub fn latest_block(url: String) -> Self {
        Self {
            url,
            blocknumber: None,
        }
    }
}

// Used by the EVM to access storage.  This can either be an in-memory
// only db or a forked db.
// The EVM delegates transact() and transact_commit to this mod...
//
// This is based heavily on Foundry's approach.
pub struct StorageBackend {
    mem_db: SimularEvmInMemoryDB, // impl wrapper to handle DbErrors
    forkdb: Option<Fork>,
    block_number: u64, // used to record in the snapshot...
}

impl Default for StorageBackend {
    fn default() -> Self {
        StorageBackend::new(None)
    }
}

impl StorageBackend {
    pub fn new(fork: Option<CreateFork>) -> Self {
        if let Some(fork) = fork {
            let backend = Fork::new(&fork.url, fork.blocknumber);
            Self {
                mem_db: SimularEvmInMemoryDB::default(),
                forkdb: Some(backend),
                block_number: fork.blocknumber.unwrap_or(0),
            }
        } else {
            Self {
                mem_db: SimularEvmInMemoryDB::default(),
                forkdb: None,
                block_number: 0,
            }
        }
    }

    pub fn insert_account_info(&mut self, address: Address, info: AccountInfo) {
        if let Some(fork) = self.forkdb.as_mut() {
            fork.database_mut().insert_account_info(address, info)
        } else {
            // use mem...
            self.mem_db.insert_account_info(address, info)
        }
    }

    pub fn insert_account_storage(
        &mut self,
        address: Address,
        slot: U256,
        value: U256,
    ) -> Result<(), DatabaseError> {
        let ret = if let Some(fork) = self.forkdb.as_mut() {
            fork.database_mut()
                .insert_account_storage(address, slot, value)
        } else {
            self.mem_db.insert_account_storage(address, slot, value)
        };
        ret
    }

    pub fn replace_account_storage(
        &mut self,
        address: Address,
        storage: Map<U256, U256>,
    ) -> Result<(), DatabaseError> {
        if let Some(fork) = self.forkdb.as_mut() {
            fork.database_mut()
                .replace_account_storage(address, storage)
        } else {
            self.mem_db.replace_account_storage(address, storage)
        }
    }

    pub fn run_transact(&mut self, env: &mut EnvWithHandlerCfg) -> Result<ResultAndState> {
        let mut evm = create_evm(self, env.clone());
        let res = evm
            .transact()
            .map_err(|e| anyhow!("backend failed while executing transaction:  {:?}", e))?;
        env.env = evm.context.evm.inner.env;

        Ok(res)
    }

    // TODO dedup code here...  Move create_snapshot impl to each backend...
    pub fn create_snapshot(&self) -> Result<SnapShot> {
        if let Some(db) = self.forkdb.as_ref() {
            let accounts = db
                .database()
                .accounts
                .clone()
                .into_iter()
                .map(|(k, v)| -> Result<(Address, SnapShotAccountRecord)> {
                    let code = if let Some(code) = v.info.code {
                        code
                    } else {
                        db.database().code_by_hash_ref(v.info.code_hash)?
                    }
                    .to_checked();
                    Ok((
                        k,
                        SnapShotAccountRecord {
                            nonce: v.info.nonce,
                            balance: v.info.balance,
                            code: code.original_bytes(),
                            storage: v.storage.into_iter().collect(),
                        },
                    ))
                })
                .collect::<Result<_, _>>()?;
            Ok(SnapShot {
                block_num: self.block_number,
                source: SnapShotSource::Fork,
                accounts,
            })
        } else {
            let accounts = self
                .mem_db
                .accounts
                .clone()
                .into_iter()
                .map(|(k, v)| -> Result<(Address, SnapShotAccountRecord)> {
                    let code = if let Some(code) = v.info.code {
                        code
                    } else {
                        self.mem_db.code_by_hash_ref(v.info.code_hash)?
                    }
                    .to_checked();
                    Ok((
                        k,
                        SnapShotAccountRecord {
                            nonce: v.info.nonce,
                            balance: v.info.balance,
                            code: code.original_bytes(),
                            storage: v.storage.into_iter().collect(),
                        },
                    ))
                })
                .collect::<Result<_, _>>()?;
            Ok(SnapShot {
                block_num: self.block_number,
                source: SnapShotSource::Memory,
                accounts,
            })
        }
    }

    pub fn load_snapshot(&mut self, snapshot: SnapShot) {
        self.block_number = snapshot.block_num;

        for (addr, account) in snapshot.accounts.into_iter() {
            // note: this will populate both 'accounts' and 'contracts'
            self.mem_db.insert_account_info(
                addr,
                AccountInfo {
                    balance: account.balance,
                    nonce: account.nonce,
                    code_hash: KECCAK_EMPTY,
                    code: if account.code.0.is_empty() {
                        None
                    } else {
                        Some(
                            Bytecode::new_raw(alloy_primitives::Bytes(account.code.0)).to_checked(),
                        )
                    },
                },
            );

            // ... but we still need to load the account storage map
            for (k, v) in account.storage.into_iter() {
                self.mem_db
                    .accounts
                    .entry(addr)
                    .or_default()
                    .storage
                    .insert(k, v);
            }
        }
    }
}

impl DatabaseRef for StorageBackend {
    type Error = DatabaseError;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        if let Some(db) = self.forkdb.as_ref() {
            db.basic_ref(address)
        } else {
            Ok(self.mem_db.basic_ref(address)?)
        }
    }

    fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        if let Some(db) = self.forkdb.as_ref() {
            db.code_by_hash_ref(code_hash)
        } else {
            Ok(self.mem_db.code_by_hash_ref(code_hash)?)
        }
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        if let Some(db) = self.forkdb.as_ref() {
            DatabaseRef::storage_ref(db, address, index)
        } else {
            Ok(DatabaseRef::storage_ref(&self.mem_db, address, index)?)
        }
    }

    fn block_hash_ref(&self, number: U256) -> Result<B256, Self::Error> {
        if let Some(db) = self.forkdb.as_ref() {
            db.block_hash_ref(number)
        } else {
            Ok(self.mem_db.block_hash_ref(number)?)
        }
    }
}

impl Database for StorageBackend {
    type Error = DatabaseError;
    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        if let Some(db) = self.forkdb.as_mut() {
            db.basic(address)
        } else {
            Ok(self.mem_db.basic(address)?)
        }
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        if let Some(db) = self.forkdb.as_mut() {
            db.code_by_hash(code_hash)
        } else {
            Ok(self.mem_db.code_by_hash(code_hash)?)
        }
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        if let Some(db) = self.forkdb.as_mut() {
            Database::storage(db, address, index)
        } else {
            Ok(Database::storage(&mut self.mem_db, address, index)?)
        }
    }

    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        if let Some(db) = self.forkdb.as_mut() {
            db.block_hash(number)
        } else {
            Ok(self.mem_db.block_hash(number)?)
        }
    }
}

impl DatabaseCommit for StorageBackend {
    fn commit(&mut self, changes: Map<Address, Account>) {
        if let Some(db) = self.forkdb.as_mut() {
            db.commit(changes)
        } else {
            self.mem_db.commit(changes)
        }
    }
}

fn create_evm<'a, DB: Database>(
    db: DB,
    env: revm::primitives::EnvWithHandlerCfg,
) -> revm::Evm<'a, (), DB> {
    EvmBuilder::default()
        .with_db(db)
        .with_env(env.env.clone())
        .build()
}