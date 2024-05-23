use alloy_primitives::{utils::format_ether, U256};
use simular_core::{BaseEvm, SnapShot};

mod helpers;
use helpers::*;

fn buy_dai() {
    let zero = U256::from(0);

    let snap = include_str!("./uniswap_snapshot.json");
    let snapshot: SnapShot = serde_json::from_slice(snap.as_bytes()).unwrap();
    let mut evm = BaseEvm::new_from_snapshot(snapshot);

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

    let sqrtp = evm
        .transact_call_sol(pool_address, UniswapPool::slot0Call {}, zero)
        .unwrap()
        .sqrtPriceX96;

    println!("Swapping WETH for DAI");
    println!("~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~");
    println!("Making 10 buys...");
    for _ in 0..10 {
        // single agent
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

        println!("recv: {:} DAI for 1 WETH", dai_recv);
        println!("---------------------------------");
    }
}

pub fn main() {
    //create_snapshot();
    buy_dai();
}
