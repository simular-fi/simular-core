use alloy_primitives::{Address, U256};
use alloy_sol_types::{sol, SolCall};
use anyhow::Result;
use plotters::prelude::*;
use revm::{primitives::bitvec::macros::internal::funty::Fundamental, Database, DatabaseCommit};

use simular_core::{baseevm::BaseEvm, forkdb::ForkDb, memdb::InMemoryDb, SerializableState};

sol!(Dai, "examples/abis/dai.abi");
sol!(Weth, "examples/abis/weth.abi");
sol!(SwapRouter, "examples/abis/SwapRouter.abi");
sol!(UniswapPool, "examples/abis/UniswapV3Pool.abi");
sol!(UniswapFactory, "examples/abis/UniswapV3Factory.abi");

const Q96: f64 = 79228162514264340000000000000.0;
const FEE: u32 = 3000;
const DEPOSIT: u128 = 1e24 as u128; // 1_000_000 eth

fn call<T: SolCall, DB: Database + DatabaseCommit>(
    evm: &mut BaseEvm<DB>,
    to: Address,
    args: T,
) -> Result<<T as SolCall>::Return> {
    //let (encode_get_pool, _, decoder) = abi.encode_function(fn_name, args).unwrap();
    let data = args.abi_encode();
    let (output, _) = evm.call(to, data).unwrap();
    T::abi_decode_returns(&output, true).map_err(|e| anyhow::anyhow!("error: {:?}", e))
}

fn transact<T: SolCall, DB: Database + DatabaseCommit>(
    evm: &mut BaseEvm<DB>,
    to: Address,
    caller: Address,
    args: T,
    value: U256,
) -> Result<<T as SolCall>::Return> {
    let data = args.abi_encode();
    let (output, _) = evm.transact(caller, to, data, value).unwrap();
    T::abi_decode_returns(&output, true).map_err(|e| anyhow::anyhow!("error: {:?}", e))
}

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

    let mut evm = BaseEvm::<ForkDb>::create(url, None);

    evm.create_account(agent, Some(deposit)).unwrap();
    evm.create_account(dai_admin, Some(deposit)).unwrap(); // <--- NOTE THIS

    let pool_address = call(
        &mut evm,
        uniswap_factory,
        UniswapFactory::getPoolCall {
            _0: weth,
            _1: dai,
            _2: FEE,
        },
    )
    .unwrap()
    ._0;

    let token_0 = call(&mut evm, pool_address, UniswapPool::token0Call {})
        .unwrap()
        ._0;
    let token_1 = call(&mut evm, pool_address, UniswapPool::token1Call {})
        .unwrap()
        ._0;

    // solely to load in cache
    call(&mut evm, pool_address, UniswapPool::slot0Call {}).unwrap();

    // fund/approve agent's weth account
    transact(&mut evm, weth, agent, Weth::depositCall {}, deposit).unwrap();
    transact(
        &mut evm,
        weth,
        agent,
        Weth::approveCall {
            guy: uniswap_router,
            wad: deposit,
        },
        zero,
    )
    .unwrap();

    assert_eq!(
        U256::from(1),
        call(&mut evm, dai, Dai::wardsCall { _0: dai_admin })
            .unwrap()
            ._0
    );

    // mint/approve agent's dai account
    transact(
        &mut evm,
        dai,
        dai_admin,
        Dai::mintCall {
            usr: agent,
            wad: deposit,
        },
        zero,
    )
    .unwrap();
    transact(
        &mut evm,
        dai,
        agent,
        Dai::approveCall {
            usr: uniswap_router,
            wad: deposit,
        },
        zero,
    )
    .unwrap();

    // confirm balances and allowances
    let weth_bal = call(&mut evm, weth, Weth::balanceOfCall { _0: agent }).unwrap();
    let dai_bal = call(&mut evm, weth, Dai::balanceOfCall { _0: agent }).unwrap();
    assert_eq!(weth_bal._0, deposit);
    assert_eq!(dai_bal._0, deposit);

    print_balances(&mut evm, agent, dai, weth);

    let dai_allowance = call(
        &mut evm,
        dai,
        Dai::allowanceCall {
            _0: agent,
            _1: uniswap_router,
        },
    )
    .unwrap()
    ._0;
    assert_eq!(dai_allowance, deposit);
    let weth_allowance = call(
        &mut evm,
        weth,
        Weth::allowanceCall {
            _0: agent,
            _1: uniswap_router,
        },
    )
    .unwrap()
    ._0;
    assert_eq!(weth_allowance, deposit);

    // the call that's been killing me now works!
    let swapped = transact(
        &mut evm,
        uniswap_router,
        agent,
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

    let st = evm.dump_state().unwrap();
    let jsonstate = serde_json::to_string_pretty(&st).unwrap();
    std::fs::write("wethdai_cache.json", jsonstate).expect("Unable to write file");
}

fn load_and_run_from_cache() -> (Vec<(f32, f32)>, ((f32, f32), (f64, f64))) {
    let zero = U256::from(0);
    //let deposit: U256 = U256::from(DEPOSIT);

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
    let cache: SerializableState = serde_json::from_slice(&raw).unwrap();

    let mut evm: BaseEvm<InMemoryDb> = BaseEvm::default();
    evm.load_state(cache);

    println!("starting balances");
    print_balances(&mut evm, agent, dai, weth);

    let pool_address = call(
        &mut evm,
        uniswap_factory,
        UniswapFactory::getPoolCall {
            _0: weth,
            _1: dai,
            _2: FEE,
        },
    )
    .unwrap()
    ._0;
    let token_0 = call(&mut evm, pool_address, UniswapPool::token0Call {})
        .unwrap()
        ._0;
    let token_1 = call(&mut evm, pool_address, UniswapPool::token1Call {})
        .unwrap()
        ._0;

    let sqrtp = call(&mut evm, pool_address, UniswapPool::slot0Call {})
        .unwrap()
        .sqrtPriceX96;

    let dai_initial_price = token1_price(sqrtp);
    println!("starting dai price: {}", dai_initial_price);
    println!("starting weth price: {}", token0_price(sqrtp));

    let mut data: Vec<(f32, f32)> = Vec::new();

    for i in 0..10 {
        // single agent
        let swapped = transact(
            &mut evm,
            uniswap_router,
            agent,
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

        //let sqrtp = call(&mut evm, pool_address, UniswapPool::slot0Call {})
        //    .unwrap()
        //    .sqrtPriceX96;
        //let t1_price = token1_price(sqrtp);
        //println!("dia price per weth: {:?} dai", t1_price);
        //data.push((i as f32, t1_price as f32));
    }
    print_balances(&mut evm, agent, dai, weth);

    let sqrtp = call(&mut evm, pool_address, UniswapPool::slot0Call {})
        .unwrap()
        .sqrtPriceX96;

    let dai_final_price = token1_price(sqrtp);
    println!("final dai price: {}", dai_final_price);
    println!("final weth price: {}", token0_price(sqrtp));

    let xy = ((0f32, 8f32), (dai_initial_price, dai_final_price));
    (data, xy)
}

fn print_balances<DB: Database + DatabaseCommit>(
    evm: &mut BaseEvm<DB>,
    user: Address,
    dai: Address,
    weth: Address,
) {
    let dai_bal = call(evm, dai, Dai::balanceOfCall { _0: user }).unwrap();
    println!("dia bal: {:?}", div_u256(dai_bal._0, U256::from(1e18), 12));

    let weth_bal = call(evm, weth, Weth::balanceOfCall { _0: user }).unwrap();
    println!(
        "weth bal from cache: {:?}",
        div_u256(weth_bal._0, U256::from(1e18), 12)
    );
}
pub fn main() {
    //dotenvy::dotenv().expect("env file");
    //let url = dotenvy::var("ALCHEMY").expect("Alchemy URL");
    //init_cache(&url);

    let data = load_and_run_from_cache();

    /*
    let xaxis = std::ops::Range {
        start: data.1 .0 .0,
        end: data.1 .0 .1,
    };

    let yaxis = std::ops::Range {
        start: data.1 .1 .0,
        end: data.1 .1 .1,
    };
    */

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
}
