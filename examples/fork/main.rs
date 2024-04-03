use alloy_primitives::{address, Address, U256};
use alloy_sol_types::sol;
//use plotters::prelude::*;
use revm::primitives::bitvec::macros::internal::funty::Fundamental;

use simular_core::{BaseEvm, CreateFork, SnapShot};

sol!(Dai, "examples/abis/dai.abi");
sol!(Weth, "examples/abis/weth.abi");
sol!(SwapRouter, "examples/abis/SwapRouter.abi");
sol!(UniswapPool, "examples/abis/UniswapV3Pool.abi");
sol!(UniswapFactory, "examples/abis/UniswapV3Factory.abi");

const Q96: f64 = 79228162514264340000000000000.0;
const FEE: u32 = 3000;
const DEPOSIT: u128 = 1e24 as u128; // 1_000_000 eth

/// Divide two u256 values returning a f64
///
/// # Arguments
///
/// * `x` - u256 value
/// * `y` - u256 value
/// * `precision` - decimal precision of the result
///
pub fn div_u256(x: U256, y: U256, precision: i32) -> f64 {
    let z = x * U256::from(10).pow(U256::from(precision)) / y;
    let z: u64 = z
        .clamp(U256::ZERO, U256::from(u64::MAX))
        .try_into()
        .unwrap();
    z.as_f64() / 10f64.powi(precision)
}

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

// TODO move addresses here
const HELLO: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");

#[allow(dead_code)]
fn init_cache(url: &str) {
    let zero = U256::from(0);
    let deposit: U256 = U256::from(DEPOSIT);

    // Addresses
    let weth: Address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
        .parse::<Address>()
        .unwrap();
    let dai: Address = "0x6B175474E89094C44Da98b954EedeAC495271d0F"
        .parse::<Address>()
        .unwrap();
    let dai_admin: Address = "0x9759A6Ac90977b93B58547b4A71c78317f391A28"
        .parse::<Address>()
        .unwrap();
    let uniswap_factory: Address = "0x1F98431c8aD98523631AE4a59f267346ea31F984"
        .parse::<Address>()
        .unwrap();
    let uniswap_router: Address = "0xE592427A0AEce92De3Edee1F18E0157C05861564"
        .parse::<Address>()
        .unwrap();
    let agent = Address::with_last_byte(22);

    let mut evm = BaseEvm::new(Some(CreateFork::latest_block(url.to_string())));

    evm.create_account(agent, Some(deposit)).unwrap();
    evm.create_account(dai_admin, Some(deposit)).unwrap(); // <--- NOTE THIS

    let pool_address = evm
        .transact_call_sol(
            uniswap_factory,
            UniswapFactory::getPoolCall {
                _0: weth,
                _1: dai,
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

    // solely to load in cache
    evm.transact_call_sol(pool_address, UniswapPool::slot0Call {}, zero)
        .unwrap();

    // fund/approve agent's weth account
    evm.transact_commit_sol(agent, weth, Weth::depositCall {}, deposit)
        .unwrap();
    evm.transact_commit_sol(
        agent,
        weth,
        Weth::approveCall {
            guy: uniswap_router,
            wad: deposit,
        },
        zero,
    )
    .unwrap();

    assert_eq!(
        U256::from(1),
        evm.transact_call_sol(dai, Dai::wardsCall { _0: dai_admin }, zero)
            .unwrap()
            ._0
    );

    // mint/approve agent's dai account
    evm.transact_commit_sol(
        dai_admin,
        dai,
        Dai::mintCall {
            usr: agent,
            wad: deposit,
        },
        zero,
    )
    .unwrap();

    evm.transact_commit_sol(
        agent,
        dai,
        Dai::approveCall {
            usr: uniswap_router,
            wad: deposit,
        },
        zero,
    )
    .unwrap();

    // confirm balances and allowances
    let weth_bal = evm
        .transact_call_sol(weth, Weth::balanceOfCall { _0: agent }, zero)
        .unwrap();
    let dai_bal = evm
        .transact_call_sol(weth, Dai::balanceOfCall { _0: agent }, zero)
        .unwrap();
    assert_eq!(weth_bal._0, deposit);
    assert_eq!(dai_bal._0, deposit);

    print_balances(&mut evm, agent, dai, weth);

    let dai_allowance = evm
        .transact_call_sol(
            dai,
            Dai::allowanceCall {
                _0: agent,
                _1: uniswap_router,
            },
            zero,
        )
        .unwrap()
        ._0;
    assert_eq!(dai_allowance, deposit);
    let weth_allowance = evm
        .transact_call_sol(
            weth,
            Weth::allowanceCall {
                _0: agent,
                _1: uniswap_router,
            },
            zero,
        )
        .unwrap()
        ._0;
    assert_eq!(weth_allowance, deposit);

    let swapped = evm
        .transact_commit_sol(
            agent,
            uniswap_router,
            SwapRouter::exactInputSingleCall {
                params: SwapRouter::ExactInputSingleParams {
                    tokenIn: token_1,
                    tokenOut: token_0,
                    fee: FEE,
                    recipient: agent,
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

    println!("got {:?} dai", div_u256(swapped, U256::from(1e18), 12));
    print_balances(&mut evm, agent, dai, weth);

    let st = evm.create_snapshot().unwrap();
    let jsonstate = serde_json::to_string_pretty(&st).unwrap();
    std::fs::write("wethdai_cache.json", jsonstate).expect("Unable to write file");
}

fn load_and_run_from_cache() -> (Vec<(f32, f32)>, ((f32, f32), (f64, f64))) {
    let zero = U256::from(0);

    // Addresses
    let agent = Address::with_last_byte(22);
    let weth: Address = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
        .parse::<Address>()
        .unwrap();
    let dai: Address = "0x6B175474E89094C44Da98b954EedeAC495271d0F"
        .parse::<Address>()
        .unwrap();
    //let dai_admin: Address = "0x9759A6Ac90977b93B58547b4A71c78317f391A28"
    //    .parse::<Address>()
    //    .unwrap();
    let uniswap_factory: Address = "0x1F98431c8aD98523631AE4a59f267346ea31F984"
        .parse::<Address>()
        .unwrap();
    let uniswap_router: Address = "0xE592427A0AEce92De3Edee1F18E0157C05861564"
        .parse::<Address>()
        .unwrap();

    let raw = std::fs::read("wethdai_cache.json").unwrap();
    let cache: SnapShot = serde_json::from_slice(&raw).unwrap();

    let mut evm = BaseEvm::new_from_snapshot(cache);

    println!("starting balances");
    print_balances(&mut evm, agent, dai, weth);

    let pool_address = evm
        .transact_call_sol(
            uniswap_factory,
            UniswapFactory::getPoolCall {
                _0: weth,
                _1: dai,
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

    let dai_initial_price = token1_price(sqrtp);
    println!("starting dai price: {}", dai_initial_price);
    println!("starting weth price: {}", token0_price(sqrtp));

    let mut data: Vec<(f32, f32)> = Vec::new();

    for i in 0..10 {
        // single agent
        let swapped = evm
            .transact_commit_sol(
                agent,
                uniswap_router,
                SwapRouter::exactInputSingleCall {
                    params: SwapRouter::ExactInputSingleParams {
                        tokenIn: token_1,
                        tokenOut: token_0,
                        fee: FEE,
                        recipient: agent,
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

        let recv_dai = div_u256(swapped, U256::from(1e18), 12);
        data.push((i as f32, recv_dai as f32));

        println!("recv: {:?} dai", recv_dai);
        println!("---------------------------------");
    }
    print_balances(&mut evm, agent, dai, weth);

    let sqrtp = evm
        .transact_call_sol(pool_address, UniswapPool::slot0Call {}, zero)
        .unwrap()
        .sqrtPriceX96;

    let dai_final_price = token1_price(sqrtp);
    println!("final dai price: {}", dai_final_price);
    println!("final weth price: {}", token0_price(sqrtp));

    let xy = ((0f32, 8f32), (dai_initial_price, dai_final_price));
    (data, xy)
}

fn print_balances(evm: &mut BaseEvm, user: Address, dai: Address, weth: Address) {
    let zero = U256::from(0);

    let dai_bal = evm
        .transact_call_sol(dai, Dai::balanceOfCall { _0: user }, zero)
        .unwrap();
    println!("dia bal: {:?}", div_u256(dai_bal._0, U256::from(1e18), 12));

    let weth_bal = evm
        .transact_call_sol(weth, Weth::balanceOfCall { _0: user }, zero)
        .unwrap();
    println!(
        "weth bal from cache: {:?}",
        div_u256(weth_bal._0, U256::from(1e18), 12)
    );
}
pub fn main() {
    //dotenvy::dotenv().expect("env file");
    //let url = dotenvy::var("ALCHEMY").expect("Alchemy URL");
    //init_cache(&url);

    let _data = load_and_run_from_cache();

    /*
    let xstart = 0 as f32;
    let xstop = 9 as f32;
    let ystart = data.1 .1 .0 as f32;
    let ystop = data.1 .1 .1 as f32;

    // Plot it!!
    let root = BitMapBackend::new("5.png", (640, 480)).into_drawing_area();
    root.fill(&WHITE).unwrap();
    let root = root.margin(10, 25, 25, 10);
    // After this point, we should be able to construct a chart context
    let mut chart = ChartBuilder::on(&root)
        // Set the caption of the chart
        .caption("Dai per 1 Weth", ("sans-serif", 20).into_font())
        // Set the size of the label region
        .x_label_area_size(20)
        .y_label_area_size(40)
        // Finally attach a coordinate on the drawing area and make a chart context
        .build_cartesian_2d(xstart..xstop, ystart..ystop)
        .unwrap();

    // Then we can draw a mesh
    chart
        .configure_mesh()
        // We can customize the maximum number of labels allowed for each axis
        .x_labels(10)
        .y_labels(10)
        // We can also change the format of the label text
        //.y_label_formatter(&|x| format!("{:.3}", x))
        //.x_desc("purchases")
        //.y_desc("dia per 1 eth")
        .draw()
        .unwrap();

    // And we can draw something in the drawing area
    chart.draw_series(LineSeries::new(data.0, &RED)).unwrap();
    // Similarly, we can draw point series
    //chart
    //    .draw_series(PointSeries::of_element(data, 5, &RED, &|c, s, st| {
    //        return EmptyElement::at(c)    // We want to construct a composed element on-the-fly
    //        + Circle::new((0,0),s,st.filled()) // At this point, the new pixel coordinate is established
    //        + Text::new(format!("{:?}", c), (10, 0), ("sans-serif", 10).into_font());
    //    }))
    //    .unwrap();
    root.present().unwrap();
    */
}
