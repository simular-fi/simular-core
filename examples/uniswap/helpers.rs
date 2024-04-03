use alloy_primitives::{address, utils::format_ether, Address, U256};
use alloy_sol_types::sol;
use simular_core::{BaseEvm, CreateFork};

// Addresses used
pub const AGENT: Address = address!("2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b");
pub const DAI: Address = address!("6B175474E89094C44Da98b954EedeAC495271d0F");
pub const WETH: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
pub const DAI_ADMIN: Address = address!("9759A6Ac90977b93B58547b4A71c78317f391A28");
pub const UNISWAP_FACTORY: Address = address!("1F98431c8aD98523631AE4a59f267346ea31F984");
pub const UNISWAP_ROUTER: Address = address!("E592427A0AEce92De3Edee1F18E0157C05861564");

pub const FEE: u32 = 3000;
const Q96: f64 = 79228162514264340000000000000.0;
pub const DEPOSIT: u128 = 1e24 as u128; // 1_000_000 eth

sol!(Dai, "examples/abis/dai.abi");
sol!(Weth, "examples/abis/weth.abi");
sol!(SwapRouter, "examples/abis/SwapRouter.abi");
sol!(UniswapPool, "examples/abis/UniswapV3Pool.abi");
sol!(UniswapFactory, "examples/abis/UniswapV3Factory.abi");

pub fn sqrtp_to_price(sqrtp: U256) -> f64 {
    //let q96 = 2f64.powf(96.0);
    let sp: f64 = sqrtp.try_into().unwrap();
    (sp / Q96).powf(2.0)
}

pub fn token0_price(sqrtp: U256) -> f64 {
    let sp = sqrtp_to_price(sqrtp);
    sp
}

pub fn token1_price(sqrtp: U256) -> f64 {
    let t0 = token0_price(sqrtp);
    assert!(t0 > 0.0);
    1.0 / t0
}

/// Builds a Snapshot from a Fork of the Uniswap pair WETH/DAI. Setup
/// below is required to ensure all contract information is loaded and
/// save to the snapshot.
#[allow(dead_code)]
pub fn create_snapshot() {
    let zero = U256::from(0);
    let deposit: U256 = U256::from(DEPOSIT);

    // NOTE: expects the following env var with the URL to a json-rpc node/service
    dotenvy::dotenv().expect("env file");
    let url = dotenvy::var("ALCHEMY").expect("Alchemy URL");

    let mut evm = BaseEvm::new(Some(CreateFork::latest_block(url.to_string())));

    evm.create_account(AGENT, Some(deposit)).unwrap();
    evm.create_account(DAI_ADMIN, Some(deposit)).unwrap();

    // Make some calls to load the state
    let pool_address = evm
        .transact_call_sol(
            UNISWAP_FACTORY,
            UniswapFactory::getPoolCall {
                _0: WETH,
                _1: DAI,
                _2: FEE,
            },
            zero,
        )
        .unwrap()
        ._0;

    let token_0 = evm
        .transact_call_sol(pool_address, UniswapPool::token0Call {}, zero)
        .unwrap()
        ._0;
    let token_1 = evm
        .transact_call_sol(pool_address, UniswapPool::token1Call {}, zero)
        .unwrap()
        ._0;

    evm.transact_call_sol(pool_address, UniswapPool::slot0Call {}, zero)
        .unwrap();

    // Fund and approve account on weth and dai
    // fund/approve agent's weth account
    evm.transact_commit_sol(AGENT, WETH, Weth::depositCall {}, deposit)
        .unwrap();
    evm.transact_commit_sol(
        AGENT,
        WETH,
        Weth::approveCall {
            guy: UNISWAP_ROUTER,
            wad: deposit,
        },
        zero,
    )
    .unwrap();

    // mint/approve agent's dai account
    evm.transact_commit_sol(
        DAI_ADMIN,
        DAI,
        Dai::mintCall {
            usr: AGENT,
            wad: deposit,
        },
        zero,
    )
    .unwrap();

    evm.transact_commit_sol(
        AGENT,
        DAI,
        Dai::approveCall {
            usr: UNISWAP_ROUTER,
            wad: deposit,
        },
        zero,
    )
    .unwrap();

    // confirm balances and allowances
    let weth_bal = evm
        .transact_call_sol(WETH, Weth::balanceOfCall { _0: AGENT }, zero)
        .unwrap();
    let dai_bal = evm
        .transact_call_sol(WETH, Dai::balanceOfCall { _0: AGENT }, zero)
        .unwrap();
    assert_eq!(weth_bal._0, deposit);
    assert_eq!(dai_bal._0, deposit);

    // Make allowance calls for both Weth and DAI
    let dai_allowance = evm
        .transact_call_sol(
            DAI,
            Dai::allowanceCall {
                _0: AGENT,
                _1: UNISWAP_ROUTER,
            },
            zero,
        )
        .unwrap()
        ._0;
    assert_eq!(dai_allowance, deposit);

    let weth_allowance = evm
        .transact_call_sol(
            WETH,
            Weth::allowanceCall {
                _0: AGENT,
                _1: UNISWAP_ROUTER,
            },
            zero,
        )
        .unwrap()
        ._0;
    assert_eq!(weth_allowance, deposit);

    let swapped = evm
        .transact_commit_sol(
            AGENT,
            UNISWAP_ROUTER,
            SwapRouter::exactInputSingleCall {
                params: SwapRouter::ExactInputSingleParams {
                    tokenIn: token_1,
                    tokenOut: token_0,
                    fee: FEE,
                    recipient: AGENT,
                    deadline: U256::from(1e32 as u128),
                    amountIn: U256::from(1e18 as u128),
                    amountOutMinimum: U256::from(0),
                    sqrtPriceLimitX96: U256::from(0),
                },
            },
            zero,
        )
        .unwrap()
        .amountOut;

    let dai_recv = format_ether(swapped);

    println!("recv: {:?} dai", dai_recv);

    let st = evm.create_snapshot().unwrap();
    let jsonstate = serde_json::to_string_pretty(&st).unwrap();
    std::fs::write("uniswap_snapshot.json", jsonstate).expect("Unable to write file");
}
