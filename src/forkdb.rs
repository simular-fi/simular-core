use alloy_primitives::{Address, Log, U256};
use anyhow::Result;
use ethers_core::types::{Block, BlockId, TxHash, H160, H256, U64};
use ethers_providers::{Http, Middleware, Provider, ProviderError};
use revm::{
    db::{in_memory_db::DbAccount, AccountState, Database, DatabaseCommit},
    primitives::{Account, AccountInfo, Bytecode, B256, KECCAK_EMPTY},
};
use std::{
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};
use tokio::runtime::{Builder, Handle, RuntimeFlavor};

use crate::errors::DatabaseError;

pub type HttpProvider = Provider<Http>;

pub struct ForkDb {
    pub accounts: HashMap<Address, DbAccount>,
    pub contracts: HashMap<B256, Bytecode>,
    pub logs: Vec<Log>,
    pub block_hashes: HashMap<U256, B256>,
    pub block_number: u64,
    provider: Arc<HttpProvider>,
}

impl ForkDb {
    pub fn new(url: &str, starting_block_number: Option<u64>) -> Self {
        let client = Provider::<Http>::try_from(url).expect("ForkDb: failed to load HTTP provider");
        let provider = Arc::new(client);

        let block_number = if let Some(bn) = starting_block_number {
            bn
        } else {
            Self::block_on(provider.get_block_number())
                .expect("ForkDb: failed to load latest blocknumber from remote")
                .as_u64()
        };

        let mut contracts = HashMap::new();
        contracts.insert(KECCAK_EMPTY, Bytecode::new());
        contracts.insert(B256::ZERO, Bytecode::new());

        Self {
            accounts: HashMap::new(),
            contracts,
            logs: Vec::default(),
            block_hashes: HashMap::new(),
            provider,
            block_number,
        }
    }

    // adapted from revm ethersdb
    #[inline]
    fn block_on<F>(f: F) -> F::Output
    where
        F: core::future::Future + Send,
        F::Output: Send,
    {
        match Handle::try_current() {
            Ok(handle) => match handle.runtime_flavor() {
                // This essentially equals to tokio::task::spawn_blocking because tokio doesn't
                // allow current_thread runtime to block_in_place
                RuntimeFlavor::CurrentThread => std::thread::scope(move |s| {
                    s.spawn(move || {
                        Builder::new_current_thread()
                            .enable_all()
                            .build()
                            .unwrap()
                            .block_on(f)
                    })
                    .join()
                    .unwrap()
                }),
                _ => tokio::task::block_in_place(move || handle.block_on(f)),
            },
            Err(_) => Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(f),
        }
    }

    pub fn insert_contract(&mut self, account: &mut AccountInfo) {
        if let Some(code) = &account.code {
            if !code.is_empty() {
                if account.code_hash == KECCAK_EMPTY {
                    account.code_hash = code.hash_slow();
                }
                self.contracts
                    .entry(account.code_hash)
                    .or_insert_with(|| code.clone());
            }
        }
        if account.code_hash == B256::ZERO {
            account.code_hash = KECCAK_EMPTY;
        }
    }

    pub fn insert_account_info(&mut self, address: Address, mut info: AccountInfo) {
        self.insert_contract(&mut info);
        self.accounts.entry(address).or_default().info = info;
    }

    pub fn load_account(&mut self, address: Address) -> Result<&mut DbAccount, DatabaseError> {
        match self.accounts.entry(address) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(_) => Err(DatabaseError::GetAccount(address)),
        }
    }

    pub fn insert_account_storage(
        &mut self,
        address: Address,
        slot: U256,
        value: U256,
    ) -> Result<(), DatabaseError> {
        let account = self.load_account(address)?;
        account.storage.insert(slot, value);
        Ok(())
    }

    pub fn replace_account_storage(
        &mut self,
        address: Address,
        storage: HashMap<U256, U256>,
    ) -> Result<(), DatabaseError> {
        let account = self.load_account(address)?;
        account.account_state = AccountState::StorageCleared;
        account.storage = storage.into_iter().collect();
        Ok(())
    }

    fn fetch_basic_from_fork(
        provider: &HttpProvider,
        block_number: u64,
        address: Address,
    ) -> Result<AccountInfo, ProviderError> {
        let add = H160::from(address.0 .0);
        let bn: Option<BlockId> = Some(BlockId::from(block_number));

        let f = async {
            let nonce = provider.get_transaction_count(add, bn);
            let balance = provider.get_balance(add, bn);
            let code = provider.get_code(add, bn);
            tokio::join!(nonce, balance, code)
        };
        let (nonce, balance, code) = Self::block_on(f);

        let balance = U256::from_limbs(balance?.0);
        let nonce = nonce?.as_u64();
        let bytecode = Bytecode::new_raw(code?.0.into());
        let code_hash = bytecode.hash_slow();
        Ok(AccountInfo::new(balance, nonce, code_hash, bytecode))
    }

    fn fetch_storage_from_fork(
        provider: &HttpProvider,
        block_number: u64,
        address: Address,
        index: U256,
    ) -> Result<U256, ProviderError> {
        let add = H160::from(address.0 .0);
        let bn: Option<BlockId> = Some(BlockId::from(block_number));

        let index = H256::from(index.to_be_bytes());
        let slot_value: H256 = Self::block_on(provider.get_storage_at(add, index, bn))?;
        Ok(U256::from_be_bytes(slot_value.to_fixed_bytes()))
    }

    fn fetch_blockhash_from_fork(
        provider: &HttpProvider,
        number: U256,
    ) -> Result<B256, ProviderError> {
        if number > U256::from(u64::MAX) {
            return Ok(KECCAK_EMPTY);
        }
        // We know number <= u64::MAX so unwrap is safe
        let number = U64::from(u64::try_from(number).unwrap());
        let block: Option<Block<TxHash>> =
            Self::block_on(provider.get_block(BlockId::from(number)))?;
        Ok(B256::new(block.unwrap().hash.unwrap().0))
    }
}

impl Database for ForkDb {
    type Error = DatabaseError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let basics = match self.accounts.entry(address) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let info =
                    Self::fetch_basic_from_fork(self.provider.as_ref(), self.block_number, address);
                let account = match info {
                    Ok(i) => DbAccount {
                        info: i,
                        ..Default::default()
                    },
                    Err(_) => DbAccount::new_not_existing(),
                };
                entry.insert(account)
            }
        };
        Ok(basics.info())
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        match self.contracts.entry(code_hash) {
            Entry::Occupied(entry) => Ok(entry.get().clone()),
            Entry::Vacant(_) => Err(DatabaseError::MissingCode(code_hash)),
        }
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        match self.accounts.entry(address) {
            Entry::Occupied(mut acc_entry) => {
                let acc_entry = acc_entry.get_mut();
                match acc_entry.storage.entry(index) {
                    Entry::Occupied(entry) => Ok(*entry.get()),
                    Entry::Vacant(entry) => {
                        if matches!(
                            acc_entry.account_state,
                            AccountState::StorageCleared | AccountState::NotExisting
                        ) {
                            Ok(U256::ZERO)
                        } else {
                            let slot = Self::fetch_storage_from_fork(
                                self.provider.as_ref(),
                                self.block_number,
                                address,
                                index,
                            );
                            match slot {
                                Ok(s) => {
                                    entry.insert(s);
                                    Ok(s)
                                }
                                Err(_) => Err(DatabaseError::GetStorage(address, index)),
                            }
                        }
                    }
                }
            }
            Entry::Vacant(_) => Err(DatabaseError::GetAccount(address)),
        }
    }

    fn block_hash(&mut self, number: U256) -> Result<B256, Self::Error> {
        match self.block_hashes.entry(number) {
            Entry::Occupied(entry) => Ok(*entry.get()),
            Entry::Vacant(entry) => {
                let hash = Self::fetch_blockhash_from_fork(self.provider.as_ref(), number);
                match hash {
                    Ok(h) => {
                        entry.insert(h);
                        Ok(h)
                    }
                    Err(_) => Err(DatabaseError::GetBlockHash(number)),
                }
            }
        }
    }
}

impl DatabaseCommit for ForkDb {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        for (address, mut account) in changes {
            if !account.is_touched() {
                continue;
            }
            if account.is_selfdestructed() {
                let db_account = self.accounts.entry(address).or_default();
                db_account.storage.clear();
                db_account.account_state = AccountState::NotExisting;
                db_account.info = AccountInfo::default();
                continue;
            }
            let is_newly_created = account.is_created();
            self.insert_contract(&mut account.info);

            let db_account = self.accounts.entry(address).or_default();
            db_account.info = account.info;

            db_account.account_state = if is_newly_created {
                db_account.storage.clear();
                AccountState::StorageCleared
            } else if db_account.account_state.is_storage_cleared() {
                // Preserve old account state if it already exists
                AccountState::StorageCleared
            } else {
                AccountState::Touched
            };
            db_account.storage.extend(
                account
                    .storage
                    .into_iter()
                    .map(|(key, value)| (key, value.present_value())),
            );
        }
    }
}
