use alloy_sol_types::decode_revert_reason;
use anyhow::Result;
use revm::{
    primitives::{
        result::EVMError, AccountInfo, Address, Bytecode, ExecutionResult, Log, Output,
        ResultAndState, TransactTo, TxEnv, KECCAK_EMPTY, U256,
    },
    ContextWithHandlerCfg, Database, DatabaseCommit, Evm, Handler,
};

use crate::{
    forkdb::ForkDb,
    memdb::InMemoryDb,
    snapshot::{SerializableAccountRecord, SerializableState, SerializingSource},
};

/// Simpler types for both versions of the Evm
pub type EvmFork = BaseEvm<ForkDb>;
pub type EvmMemory = BaseEvm<InMemoryDb>;

/// Provides main interaction with the under-lying EVM
pub struct BaseEvm<DB: Database + DatabaseCommit> {
    state: Option<ContextWithHandlerCfg<(), DB>>,
}

/// Create an EVM that will pull missing information from a remote json-rpc endpoint.
/// - `url` is the url to access the endpoint
/// - `block_number` where to start. if none, it will use the latest block
impl EvmFork {
    pub fn create(url: &str, block_number: Option<u64>) -> Self {
        let db = ForkDb::new(url, block_number);
        let evm = Evm::builder().with_db(db).build();
        Self {
            state: Some(evm.into_context_with_handler_cfg()),
        }
    }

    /// Create an account for the `caller` with an optional balance in `wei`
    pub fn create_account(&mut self, caller: Address, amount: Option<U256>) -> Result<()> {
        let mut info = AccountInfo::default();
        if let Some(amnt) = amount {
            info.balance = amnt;
        }
        let mut evm = self.get_evm();
        evm.context.evm.db.insert_account_info(caller, info);
        self.state = Some(evm.into_context_with_handler_cfg());

        Ok(())
    }

    /// Dump the current state of the db to Json
    pub fn dump_state(&mut self) -> Result<SerializableState> {
        let mut evm = self.get_evm();
        // adapted from foundry-rs
        let blknum = evm.context.evm.db.block_number;
        let accounts = evm
            .context
            .evm
            .db
            .accounts
            .clone()
            .into_iter()
            .map(|(k, v)| -> Result<(Address, SerializableAccountRecord)> {
                let code = if let Some(code) = v.info.code {
                    code
                } else {
                    evm.context.evm.db.code_by_hash(v.info.code_hash)?
                }
                .to_checked();
                Ok((
                    k,
                    SerializableAccountRecord {
                        nonce: v.info.nonce,
                        balance: v.info.balance,
                        code: code.original_bytes(),
                        storage: v.storage.into_iter().collect(),
                    },
                ))
            })
            .collect::<Result<_, _>>()?;

        self.state = Some(evm.into_context_with_handler_cfg());

        Ok(SerializableState {
            block_num: blknum,
            source: SerializingSource::Fork,
            accounts,
        })
    }
}

impl Default for EvmMemory {
    fn default() -> Self {
        let db = InMemoryDb::default();
        let evm = Evm::builder().with_db(db).build();
        Self {
            state: Some(evm.into_context_with_handler_cfg()),
        }
    }
}

impl EvmMemory {
    pub fn create_account(&mut self, caller: Address, amount: Option<U256>) -> Result<()> {
        let mut info = AccountInfo::default();
        if let Some(amnt) = amount {
            info.balance = amnt;
        }
        let mut evm = self.get_evm();
        evm.context.evm.db.insert_account_info(caller, info);
        self.state = Some(evm.into_context_with_handler_cfg());

        Ok(())
    }

    pub fn dump_state(&mut self) -> Result<SerializableState> {
        let mut evm = self.get_evm();
        let blknum = evm.context.evm.db.block_number;
        // adapted from foundry-rs
        let accounts = evm
            .context
            .evm
            .db
            .accounts
            .clone()
            .into_iter()
            .map(|(k, v)| -> Result<(Address, SerializableAccountRecord)> {
                let code = if let Some(code) = v.info.code {
                    code
                } else {
                    evm.context.evm.db.code_by_hash(v.info.code_hash)?
                }
                .to_checked();
                Ok((
                    k,
                    SerializableAccountRecord {
                        nonce: v.info.nonce,
                        balance: v.info.balance,
                        code: code.original_bytes(),
                        storage: v.storage.into_iter().collect(),
                    },
                ))
            })
            .collect::<Result<_, _>>()?;

        self.state = Some(evm.into_context_with_handler_cfg());

        Ok(SerializableState {
            block_num: blknum,
            source: SerializingSource::Memory,
            accounts,
        })
    }

    /// Load state from a json file
    pub fn load_state(&mut self, cache: SerializableState) {
        let mut evm = self.get_evm();
        evm.context.evm.db.set_blocknumber(cache.block_num);

        for (addr, account) in cache.accounts.into_iter() {
            // note: this will populate both 'accounts' and 'contracts'
            evm.context.evm.db.insert_account_info(
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
                evm.context
                    .evm
                    .db
                    .accounts
                    .entry(addr)
                    .or_default()
                    .storage
                    .insert(k, v);
            }
        }
        self.state = Some(evm.into_context_with_handler_cfg());
    }
}

impl<DB: Database + DatabaseCommit> BaseEvm<DB> {
    fn get_evm(&mut self) -> Evm<(), DB> {
        match self.state.take() {
            Some(st) => {
                let ContextWithHandlerCfg { context, cfg } = st;
                Evm {
                    context,
                    handler: Handler::new(cfg),
                }
            }
            _ => panic!("EVM state is None"),
        }
    }

    /// View an internal storage slot for the given address and slot index
    pub fn view_storage_slot(&mut self, addr: Address, slot: U256) -> Result<U256> {
        let mut evm = self.get_evm();
        let r = evm
            .context
            .evm
            .db
            .storage(addr, slot)
            .map_err(|_| anyhow::anyhow!("Error viewing storage slot"))?;

        self.state = Some(evm.into_context_with_handler_cfg());
        Ok(r)
    }

    /// Return the balance of the given address
    pub fn get_balance(&mut self, caller: Address) -> Result<U256> {
        let mut evm = self.get_evm();
        let result = match evm.context.evm.db.basic(caller) {
            Ok(Some(account)) => account.balance,
            _ => U256::ZERO,
        };

        self.state = Some(evm.into_context_with_handler_cfg());
        Ok(result)
    }

    /// Deploy a contract. Where:
    ///
    /// - `caller` is the creator (msg.sender)
    /// - `bincode` is the constructor arguments + contract bytecode
    /// - `value` in `wei` to send to the contract.  Note the call will revert if
    /// the constructor is not marked as `payable`.
    ///
    /// Returns the address of the deployed contract
    pub fn deploy(&mut self, caller: Address, bincode: Vec<u8>, value: U256) -> Result<Address> {
        let tx = TxEnv {
            caller,
            transact_to: TransactTo::create(),
            data: bincode.into(),
            value,
            ..Default::default()
        };

        let mut evm = self.get_evm();
        evm.context.evm.env.tx = tx;

        let r = evm.transact_commit();
        self.state = Some(evm.into_context_with_handler_cfg());
        match r {
            Ok(result) => {
                let (output, _gas, _logs) = process_execution_result(result)?;
                match output {
                    Output::Create(_, Some(address)) => Ok(address),
                    _ => Err(anyhow::anyhow!("Error on deploy: expected a create call")),
                }
            }
            _ => Err(anyhow::anyhow!("DEPLOY: EVM error")),
        }
    }

    /// Transfer value between two accounts. If the 'to' address is a contract, the should contract
    /// should have a [receive' or 'fallback](https://docs.soliditylang.org/en/latest/contracts.html#special-functions)
    pub fn transfer(&mut self, caller: Address, to: Address, amount: U256) -> Result<u64> {
        let tx = TxEnv {
            caller,
            transact_to: TransactTo::Call(to),
            value: amount,
            ..Default::default()
        };

        let mut evm = self.get_evm();
        evm.context.evm.env.tx = tx;

        let r = evm.transact_commit();
        self.state = Some(evm.into_context_with_handler_cfg());
        match r {
            Ok(result) => {
                let (_b, gas, _logs) = process_result_with_value(result)?;
                Ok(gas)
            }
            _ => Err(anyhow::anyhow!("TRANSFER: EVM error")),
        }
    }

    /// Send a write transaction `to` the given contract
    pub fn transact(
        &mut self,
        caller: Address,
        to: Address,
        data: Vec<u8>,
        value: U256,
    ) -> Result<(Vec<u8>, u64)> {
        let tx = TxEnv {
            caller,
            transact_to: TransactTo::Call(to),
            data: data.into(),
            value,
            ..Default::default()
        };

        let mut evm = self.get_evm();
        evm.context.evm.env.tx = tx;

        let r = evm.transact_commit();
        self.state = Some(evm.into_context_with_handler_cfg());
        match r {
            Ok(result) => {
                let (b, gas, _logs) = process_result_with_value(result)?;
                Ok((b, gas))
            }
            Err(e) => match e {
                EVMError::Transaction(t) => {
                    Err(anyhow::anyhow!("TRANSACT: EVM Transaction error {:?}", t))
                }
                EVMError::Database(_) => Err(anyhow::anyhow!("TRANSACT: EVM Database error")),
                EVMError::Header(h) => Err(anyhow::anyhow!("TRANSACT: EVM Header error {:?}", h)),
                EVMError::Custom(c) => Err(anyhow::anyhow!("TRANSACT: EVM Custom error {:?}", c)),
            },
        }
    }

    /// Make a read-only (view) call `to` the given contract
    pub fn call(&mut self, to: Address, data: Vec<u8>) -> Result<(Vec<u8>, u64)> {
        let tx = TxEnv {
            transact_to: TransactTo::Call(to),
            data: data.into(),
            ..Default::default()
        };
        self.handle_call_or_simulate(tx)
    }

    /// Simulate executing a transaction without changing state
    pub fn simulate(
        &mut self,
        caller: Address,
        to: Address,
        data: Vec<u8>,
    ) -> Result<(Vec<u8>, u64)> {
        let tx = TxEnv {
            caller,
            transact_to: TransactTo::Call(to),
            data: data.into(),
            ..Default::default()
        };
        self.handle_call_or_simulate(tx)
    }

    // run call/simulate based on the Tx
    fn handle_call_or_simulate(&mut self, tx: TxEnv) -> Result<(Vec<u8>, u64)> {
        let mut evm = self.get_evm();
        evm.context.evm.env.tx = tx;

        let r = evm.transact();
        self.state = Some(evm.into_context_with_handler_cfg());
        match r {
            Ok(ResultAndState { result, .. }) => {
                let (r, gas, _) = process_result_with_value(result)?;
                Ok((r, gas))
            }
            Err(_e) => Err(anyhow::anyhow!("CALL/SIMULATE: EVM error")),
        }
    }
}

//* Helpers below */
/// helper to extract results, also parses any revert message into a readable format
fn process_execution_result(result: ExecutionResult) -> Result<(Output, u64, Vec<Log>)> {
    match result {
        ExecutionResult::Success {
            output,
            gas_used,
            logs,
            ..
        } => Ok((output, gas_used, logs)),
        ExecutionResult::Revert { output, .. } => {
            match decode_revert_reason(&output) {
                Some(reason) => anyhow::bail!("Revert: {:?}", reason),
                _ => anyhow::bail!("Revert with no reason"),
            }
            //let msg = parse_revert_message(output)?;
            //anyhow::bail!("Call reverted. Reason: {:?}", msg)
        }
        ExecutionResult::Halt { reason, .. } => anyhow::bail!("Halted: {:?}", reason),
    }
}

fn process_result_with_value(result: ExecutionResult) -> Result<(Vec<u8>, u64, Vec<Log>)> {
    let (output, gas_used, logs) = process_execution_result(result)?;
    let bits = match output {
        Output::Call(value) => value,
        _ => anyhow::bail!("Failed to process results of call: Expected call output"),
    };

    Ok((bits.to_vec(), gas_used, logs))
}

#[cfg(test)]
mod tests {
    use crate::generate_random_addresses;
    use alloy_sol_types::{sol, SolCall};

    use super::*;

    #[test]
    fn balance_transfer() {
        let one_eth = U256::from(1e18);
        let addresses = generate_random_addresses(2);
        let bob = addresses[0];
        let alice = addresses[1];

        let mut evm = EvmMemory::default();
        evm.create_account(bob, Some(U256::from(2e18))).unwrap();
        evm.create_account(alice, None).unwrap();

        assert!(evm.transfer(alice, bob, one_eth).is_err()); // alice has nothing to transfer...yet
        assert!(evm.transfer(bob, alice, one_eth).is_ok());

        assert!(evm.get_balance(bob).unwrap() == one_eth);
        assert!(evm.get_balance(alice).unwrap() == one_eth);
    }

    #[test]
    fn deploy_and_interact() {
        // TODO use the DAI contract: https://etherscan.io/token/0x6b175474e89094c44da98b954eedeac495271d0f
        //let evm = EvmMemory::default();
    }
}
