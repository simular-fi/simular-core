 This library provides a simple API for interacting with an embedded Ethereum Virtual Machine (EVM).

 [![crates.io](https://img.shields.io/crates/v/simular-core?style=flat)](https://crates.io/crates/simular-core)

 ```toml
 [dependencies]
 simular-core = "0.2.5"
 ```

 # Supports
 - Both an in-memory database and the ability to fork state from a remote node
 - Snapshot of EVM state for later use
 - Both raw encoding/decoding of function calls as well as [alloy SolTypes](https://docs.rs/alloy-sol-macro/0.7.0/alloy_sol_macro/macro.sol.html)
 - Lightweight creation of ABI from [human-readable](https://docs.ethers.org/v5/api/utils/abi/formats/#abi-formats--human-readable-abi) syntax


 # Examples

## Create and interact with the EVM

   ```rust
     use simular_core::{BaseEvm, generate_random_addresses};
     use alloy_primitives::{Address, U256};

     // Generate some random addresses
     let addresses = generate_random_addresses(2);
     let bob = addresses[0];
     let alice = addresses[1];

     // create the EVM with in-memory database (default)
     let mut evm = BaseEvm::default();
     
     // create 2 accounts. Bob w/ 2 ether, alice w/ none
     evm.create_account(bob, Some(U256::from(2e18))).unwrap();
     evm.create_account(alice, None).unwrap();

     // check the balances
     assert!(evm.get_balance(bob).unwrap() == U256::from(2e18));
     assert!(evm.get_balance(alice).unwrap() == U256::from(0));
   ```

## Fork a remote contract  
Interacting with a remote contract pulls the state of the remote contract into the local in-memory database for use.

   ```rust
     use simular_core::{BaseEvm, generate_random_addresses, ContractAbi};
     use alloy_primitives::{Address, U256, address};
     
     // create ABI inline
     let abi = ContractAbi::from_human_readable(vec![
     "function totalSupply() (uint256)"
     ]);
    
     // create a fork using the latest block
     let fork_info = CreateFork.latest_block(URL OF JSON-RPC NODE);
     let mut evm = BaseEvm::new(Some(fork_info));

     // remote contract address.
     // using DAI: 0x6B175474E89094C44Da98b954EedeAC495271d0F
     let contract_address = address!("6B175474E89094C44Da98b954EedeAC495271d0F");
     
     // encode the function call
     let (encoded_total_supply, _, decoder) =
         abi.encode_function("totalSupply", "()").unwrap();

     // call the function on the remote contract
     let output = evm.transact_call(
         contract_address,
         encoded_total_supply,
         U256::from(0)).unwrap();

     // decode the result
     let value = decoder.unwrap().abi_decode(&output.result)
     
     println!("total supply: {:?}", value);
   ```

See [uniswap](https://github.com/simular-fi/simular-core/tree/main/examples/uniswap) for an example of using a fork and snapshot to trade a pair on Uniswap. 

To run the example:
```sh
> cargo run --example uniswap
```

## Standing on the shoulders of giants...
Thanks to the following projects for making this work possible!
- [revm](https://github.com/bluealloy/revm)
- [alloy-rs](https://github.com/alloy-rs)
- [foundry-rs](https://github.com/foundry-rs/foundry) for much of the design influence