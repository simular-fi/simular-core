use alloy_dyn_abi::{DynSolType, DynSolValue, ResolveSolType};
use alloy_json_abi::{ContractObject, Function, JsonAbi, StateMutability};
use alloy_primitives::Bytes;
use anyhow::{anyhow, bail, Result};

pub struct ContractAbi {
    pub abi: JsonAbi,
    pub bytecode: Option<Bytes>,
}

impl ContractAbi {
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

    pub fn from_abi_bytecode(raw: &str, bytecode: Option<Vec<u8>>) -> Self {
        let abi = serde_json::from_str::<JsonAbi>(raw).expect("parsing abi input");
        Self {
            abi,
            bytecode: bytecode.map(Bytes::from),
        }
    }

    pub fn from_human_readable(input: Vec<&str>) -> Self {
        let abi = JsonAbi::parse(input).expect("valid solidity functions information");
        Self {
            abi,
            bytecode: None,
        }
    }

    /// Is there a function with the given name?
    /// Called from the Python Contract's __getattr__
    pub fn has_function(&self, name: &str) -> bool {
        self.abi.functions.contains_key(name)
    }

    pub fn has_fallback(&self) -> bool {
        self.abi.fallback.is_some()
    }

    pub fn has_receive(&self) -> bool {
        self.abi.receive.is_some()
    }

    /// Return the contract bytecode as a Vec
    pub fn bytecode(&self) -> Option<Vec<u8>> {
        self.bytecode.as_ref().map(|b| b.to_vec())
    }

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
        // TODO fix error message
        let dynavalues = ty.coerce_str(args).map_err(|_| anyhow!("HERE error..."))?;
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
        ty.coerce_str(args)
            .map_err(|_| anyhow!("Error coercing the arguments for the function call"))
    }

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
