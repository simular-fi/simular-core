//!
//! Parse contract ABIs to encode, decode contract calls
//!
use alloy_dyn_abi::{DynSolType, DynSolValue, ResolveSolType};
use alloy_json_abi::{ContractObject, Function, JsonAbi, StateMutability};
use alloy_primitives::Bytes;
use anyhow::{anyhow, bail, Result};

pub struct ContractAbi {
    /// alloy's json abi object
    pub abi: JsonAbi,
    /// optional contract bytecode
    pub bytecode: Option<Bytes>,
}

impl ContractAbi {
    /// Parse the `abi` and `bytecode` from a compiled contract's json file.
    /// Note: `raw` is un-parsed json.
    pub fn from_full_json(raw: &str) -> Self {
        let co = serde_json::from_str::<ContractObject>(raw).expect("parsing abi json");
        if co.abi.is_none() {
            panic!("ABI not found in file")
        }
        if co.bytecode.is_none() {
            panic!("Bytecode not found in file")
        }
        Self {
            abi: co.abi.unwrap(),
            bytecode: co.bytecode,
        }
    }

    /// Parse the `abi` and `bytecode`
    /// Note: `raw` is un-parsed json.
    pub fn from_abi_bytecode(raw: &str, bytecode: Option<Vec<u8>>) -> Self {
        let abi = serde_json::from_str::<JsonAbi>(raw).expect("parsing abi input");
        Self {
            abi,
            bytecode: bytecode.map(Bytes::from),
        }
    }

    /// Parse an ABI (without bytecode) from a `Vec` of contract function definitions.
    /// See [human readable abi](https://docs.ethers.org/v5/api/utils/abi/formats/#abi-formats--human-readable-abi)
    pub fn from_human_readable(input: Vec<&str>) -> Self {
        let abi = JsonAbi::parse(input).expect("valid solidity functions information");
        Self {
            abi,
            bytecode: None,
        }
    }

    /// Is there a function with the given name?
    pub fn has_function(&self, name: &str) -> bool {
        self.abi.functions.contains_key(name)
    }

    /// Does the ABI have a fallback?
    pub fn has_fallback(&self) -> bool {
        self.abi.fallback.is_some()
    }

    /// Does the ABI have a receive?
    pub fn has_receive(&self) -> bool {
        self.abi.receive.is_some()
    }

    /// Return the contract bytecode as a Vec
    pub fn bytecode(&self) -> Option<Vec<u8>> {
        self.bytecode.as_ref().map(|b| b.to_vec())
    }

    /// Encode the information needed to create a contract.  This will
    /// concatenate the contract bytecode with any arguments required by
    /// the constructor.  Note: `args` is a string of input arguments.  See
    /// `encode_function` for more information.
    pub fn encode_constructor(&self, args: &str) -> Result<(Vec<u8>, bool)> {
        let bytecode = match self.bytecode() {
            Some(b) => b,
            _ => bail!("Missing contract bytecode!"),
        };

        let constructor = match &self.abi.constructor {
            Some(c) => c,
            _ => return Ok((bytecode, false)),
        };

        let types = constructor
            .inputs
            .iter()
            .map(|i| i.resolve().unwrap())
            .collect::<Vec<_>>();

        let ty = DynSolType::Tuple(types);
        let dynavalues = ty.coerce_str(args).map_err(|_| {
            anyhow!("Error coercing the arguments for the constructor. Check the input argument(s)")
        })?;
        let encoded_args = dynavalues.abi_encode_params();
        let is_payable = matches!(constructor.state_mutability, StateMutability::Payable);

        Ok(([bytecode, encoded_args].concat(), is_payable))
    }

    fn extract(funcs: &Function, args: &str) -> Result<DynSolValue> {
        let types = funcs
            .inputs
            .iter()
            .map(|i| i.resolve().unwrap())
            .collect::<Vec<_>>();
        let ty = DynSolType::Tuple(types);
        ty.coerce_str(args).map_err(|_| {
            anyhow!(
                "Error coercing the arguments for the function call. Check the input argument(s)"
            )
        })
    }

    /// Encode function information for use in a transaction. Note: `args` is a string
    /// of input parameters that are parsed by alloy `DynSolType`'s  and converted into
    /// `DynSolValue`s.   See [DynSolType.coerce_str()](https://docs.rs/alloy-dyn-abi/latest/alloy_dyn_abi/enum.DynSolType.html#method.coerce_str)
    ///  
    /// - `name` is the name of the function
    /// - `args` string of input arguments
    ///
    /// ## Example
    ///
    /// `"(1, hello, (0x11111111111111111111111111111, 5))"`
    ///
    /// is parsed into an alloy `DynSolValue` ...tuple, U256, etc...
    ///
    /// Returns a tuple with:
    /// - encoded function and args
    /// - whether the function is payable
    /// - and the output `DynSolType` that can be used to decode the result of the call
    pub fn encode_function(
        &self,
        name: &str,
        args: &str,
    ) -> anyhow::Result<(Vec<u8>, bool, DynSolType)> {
        let funcs = match self.abi.function(name) {
            Some(funcs) => funcs,
            _ => bail!("Function {} not found in the ABI!", name),
        };

        for f in funcs {
            let result = Self::extract(f, args);
            let is_payable = matches!(f.state_mutability, StateMutability::Payable);
            // find the first function that matches the input args
            if result.is_ok() {
                let types = f
                    .outputs
                    .iter()
                    .map(|i| i.resolve().unwrap())
                    .collect::<Vec<_>>();
                let ty = DynSolType::Tuple(types);
                let selector = f.selector().to_vec();
                let encoded_args = result.unwrap().abi_encode_params();
                let all = [selector, encoded_args].concat();

                return Ok((all, is_payable, ty));
            }
        }

        // if we get here, it means we didn't find a function that
        // matched the input arguments
        Err(anyhow::anyhow!(
            "Arguments to the function do not match what is expected"
        ))
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use alloy_primitives::{Address, U256};
    use alloy_sol_types::{sol, SolCall};

    sol! {

        struct HelloInput {
            uint256 value;
            address owner;
            uint160 beta;
        }

        contract HelloWorld {
            address public owner;
            function hello(HelloInput params) external returns (bool);
        }
    }

    sol! {
        contract MrOverLoads {
            function one() public returns (bool);
            function one(uint256);
            function one(address, (uint64, uint64)) public returns (address);
        }
    }

    #[test]
    fn check_constructor_encoding() {
        let input = vec!["constructor()"];
        let mut abi = ContractAbi::from_human_readable(input);
        // short-circuit internal check...
        abi.bytecode = Some(b"hello".into());

        assert!(abi.encode_constructor("()").is_ok());
        assert!(abi.encode_constructor("(1234)").is_err());
    }

    #[test]
    fn encoding_functions() {
        let hello_world = vec!["function hello(tuple(uint256, address, uint160)) (bool)"];
        let hw = ContractAbi::from_human_readable(hello_world);
        assert!(hw.has_function("hello"));

        let addy = Address::with_last_byte(24);
        let solencoded = HelloWorld::helloCall {
            params: HelloInput {
                value: U256::from(10),
                owner: addy,
                beta: U256::from(1),
            },
        }
        .abi_encode();

        assert!(hw.encode_function("bob", "()").is_err());
        assert!(hw.encode_function("hello", "(1,2").is_err());

        let (cencoded, is_payable, dtype) = hw
            .encode_function("hello", &format!("(({}, {}, {}))", 10, addy.to_string(), 1))
            .unwrap();

        assert!(!is_payable);
        assert_eq!(solencoded, cencoded);
        assert_eq!(dtype, DynSolType::Tuple(vec![DynSolType::Bool]));
    }

    #[test]
    fn encoding_overloaded_functions() {
        let overit = vec![
            "function one() (bool)",
            "function one(uint256)",
            "function one(address, (uint64, uint64)) (address)",
        ];
        let abi = ContractAbi::from_human_readable(overit);
        let addy = Address::with_last_byte(24);

        let sa = MrOverLoads::one_0Call {}.abi_encode();
        let (aa, _, _) = abi.encode_function("one", "()").unwrap();
        assert_eq!(sa, aa);

        let sb = MrOverLoads::one_1Call { _0: U256::from(1) }.abi_encode();
        let (ab, _, _) = abi.encode_function("one", "(1)").unwrap();
        assert_eq!(sb, ab);

        let sc = MrOverLoads::one_2Call {
            _0: addy,
            _1: (10u64, 11u64),
        }
        .abi_encode();
        let (ac, _, _) = abi
            .encode_function("one", &format!("({},({},{}))", addy.to_string(), 10, 11))
            .unwrap();

        assert_eq!(sc, ac);
    }
}
