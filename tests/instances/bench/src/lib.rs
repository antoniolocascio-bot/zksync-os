#![cfg(test)]
use rig::{
    alloy::{self, primitives::TxKind, rpc::types::TransactionRequest},
    ruint::aliases::U256,
    TestingFramework,
};
use std::path::PathBuf;
use zksync_os_tests_common::zksync_tx::ZKsyncTxEnvelope;

// WASM disabled for now
// #[test]
// #[ignore]
// fn memory_alloc_heavy() {
//     let mut chain = rig::Chain::empty(None);
//     let wallet = chain.random_wallet();
//
//     let c_addr = Address::from_low_u64_ne(1);
//     let c_bytes = rig::utils::load_wasm_bytecode("bench");
//     chain
//         .set_wasm_bytecode(B160::from_be_bytes(c_addr.0), &c_bytes)
//         .set_balance(
//             B160::from_be_bytes(wallet.address().0),
//             U256::from(1_000_000_000_000_000_u64),
//         );
//
//     let tx = rig::utils::tx_encoding::sign_and_encode_ethers_legacy_tx(
//         TransactionRequest::new()
//             .to(c_addr)
//             .gas(10_000_000)
//             .gas_price(1000)
//             .data(rig::utils::construct_calldata(
//                 "681aa816",
//                 &["0000000000000000000000000000000000000000000000000000000000000001"],
//             ))
//             .nonce(0),
//         &wallet,
//     );
//
//     let mut pc = rig::ProfilerConfig::new(PathBuf::from(format!(
//         "{}/os_profile.svg",
//         env!("CARGO_MANIFEST_DIR")
//     )));
//     pc.frequency_recip = 1;
//     chain.run_block(vec![tx], None, Some(pc));
// }

// WASM disabled for now
// #[test]
// #[ignore = "IWASM integer acceleration ops are invalid in the implementation"]
// fn fibish_wasm() {
//     let mut chain = rig::Chain::empty(None);
//     let wallet = chain.random_wallet();
//
//     let c_addr = Address::from_low_u64_ne(1);
//     let c_bytes = rig::utils::load_wasm_bytecode("bench");
//     chain
//         .set_wasm_bytecode(B160::from_be_bytes(c_addr.0), &c_bytes)
//         .set_balance(
//             B160::from_be_bytes(wallet.address().0),
//             U256::from(1_000_000_000_000_000_u64),
//         );
//
//     let tx = rig::utils::tx_encoding::sign_and_encode_ethers_legacy_tx(
//         TransactionRequest::new()
//             .to(c_addr)
//             .gas(1 << 27)
//             .gas_price(1000)
//             .data(rig::utils::construct_calldata(
//                 "0x70e31497",
//                 &[
//                     "0000000000000000000000000000000000000000000000000000000000000001",
//                     "0000000000000000000000000000000000000000000000000000000000000003",
//                     "0000000000000000000000000000000000000000000000000000000000000002",
//                 ],
//             ))
//             .nonce(0),
//         &wallet,
//     );
//
//     let mut pc = rig::ProfilerConfig::new(PathBuf::from(format!(
//         "{}/os_profile_fibish_wasm.svg",
//         env!("CARGO_MANIFEST_DIR")
//     )));
//     pc.frequency_recip = 1;
//     chain.run_block(vec![tx], None, Some(pc));
// }

#[test]
fn fibish_sol() {
    let mut tester = TestingFramework::new();
    let wallet = tester.random_signer();

    let c_addr = alloy::primitives::Address::from(alloy::primitives::U160::from(1));
    let c_bytes = rig::utils::load_sol_bytecode("bench", "arith");
    tester = tester
        .with_evm_contract(c_addr, &c_bytes)
        .with_balance(wallet.address(), U256::from(1_000_000_000_000_000_u64));

    let tx = TransactionRequest {
        to: Some(TxKind::Call(c_addr)),
        gas: Some(1 << 27),
        gas_price: Some(1000),
        input: rig::utils::construct_calldata(
            "0x9714e370",
            &[
                "0000000000000000000000000000000000000000000000000000000000000001",
                "0000000000000000000000000000000000000000000000000000000000000003",
                "0000000000000000000000000000000000000000000000000000000000000002",
            ],
        )
        .into(),
        nonce: Some(0),
        ..Default::default()
    };

    let tx = ZKsyncTxEnvelope::from_eth_tx_from_req(tx, wallet);

    let mut pc = rig::ProfilerConfig::new(PathBuf::from(format!(
        "{}/os_profile_fibish_sol.svg",
        env!("CARGO_MANIFEST_DIR")
    )));
    pc.frequency_recip = 1;
    let run_config = rig::chain::RunConfig {
        profiler_config: Some(pc),
        ..Default::default()
    };
    tester = tester.with_run_config(run_config);
    tester.execute_block(vec![tx]);
}

// ---------------------------------------------------------------------------
// Ported from branch alo/684-review-followups: synthetic single-block native-
// transfer throughput benchmark, sequencer-mode forward run.
//
// PORT NOTES (dev framework differs):
// - dev's `RunConfig` has no `validate_eoa_signature` / `do_prover_input_run`
//   fields and uses `profiler_config` (not `flamegraph`). The sequencer-mode
//   forward path (no per-tx `ecrecover`) is enabled by patching the rig's
//   forward run to `BasicBootloaderForwardSimulationConfig` (see
//   tests/rig/src/chain.rs), since dev hardwires the proving config otherwise.
// - `BlockOutput` is reached via `rig::zksync_os_interface::types`.
// ---------------------------------------------------------------------------

/// Approximate fixed core clock of the current benchmark host (GHz), used to
/// report a clock-independent cycles/tx figure. Override with `BENCH_CLOCK_GHZ`.
#[cfg(feature = "forward-flamegraph")]
const DEFAULT_CLOCK_GHZ: f64 = 2.4;

#[cfg(feature = "forward-flamegraph")]
fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Builds a synthetic native-transfer block: `m_total` 1-wei transfers to a
/// common target, sent round-robin from `n_accounts` funded EOAs (sender =
/// `i % n`, per-sender sequential nonces — like a real interleaved block). The
/// returned `TestingFramework` is configured for a forward-only, noDA run and
/// does not mutate chain state across `execute_block` calls, so the same block
/// can be replayed identically. Sequencer-mode (no `ecrecover`) is provided by
/// the rig patch described in this module's header.
#[cfg(feature = "forward-flamegraph")]
fn build_native_transfer_bench(
    n_accounts: usize,
    m_total: usize,
) -> (TestingFramework, Vec<ZKsyncTxEnvelope>) {
    use alloy::primitives::{Address, U160, U256 as AlloyU256};
    use rig::zk_ee::common_structs::DACommitmentScheme;

    assert!(n_accounts > 0 && m_total > 0, "N and M must be positive");

    // Common transfer target; pre-create it so transfers don't hit the
    // new-account code path.
    let target = Address::from(U160::from(0x1_0000u64));

    // Validium / noDA: a throughput bench shouldn't pay for data availability.
    let mut tester =
        TestingFramework::new().with_da_commitment_scheme(DACommitmentScheme::EmptyNoDA);
    tester.set_balance(target, U256::from(1u64));

    // Fund N sender EOAs generously (covers gas precharge + value across all txs).
    let funding = U256::from(1_000_000_000_000_000_000u64); // 1e18 wei
    let signers: Vec<_> = (0..n_accounts)
        .map(|_| {
            let s = tester.random_signer();
            tester.set_balance(s.address(), funding);
            s
        })
        .collect();

    let mut next_nonce = vec![0u64; n_accounts];
    let mut txs = Vec::with_capacity(m_total);
    for i in 0..m_total {
        let a = i % n_accounts;
        let nonce = next_nonce[a];
        next_nonce[a] += 1;
        let req = TransactionRequest {
            to: Some(TxKind::Call(target)),
            value: Some(AlloyU256::from(1u64)),
            gas: Some(100_000),
            gas_price: Some(1000),
            nonce: Some(nonce),
            ..Default::default()
        };
        txs.push(ZKsyncTxEnvelope::from_eth_tx_from_req(
            req,
            signers[a].clone(),
        ));
    }

    // Forward-only run config; `update_state_after_block_execution = false`
    // keeps the chain at the same pre-state so the block can be replayed
    // identically. Remaining fields take dev's `Default`.
    let run_config = rig::chain::RunConfig {
        profiler_config: None,
        do_riscv_run: false,
        check_storage_diff_hashes: false,
        check_revm_consistency: false,
        revm_independent_gas: false,
        update_state_after_block_execution: false,
        ..Default::default()
    };
    tester.set_run_config(Some(run_config));
    // No treasury mint needed (senders are funded directly); skipping it keeps
    // every replay identical.
    tester.disable_minting_tokens_to_treasury();

    (tester, txs)
}

/// Asserts every transfer in `output` executed successfully — a revert or
/// failure would mean the benchmark is measuring the wrong thing.
#[cfg(feature = "forward-flamegraph")]
fn assert_all_succeeded(
    output: &rig::zksync_os_interface::types::BlockOutput,
    m_total: usize,
) {
    use rig::zksync_os_interface::types::ExecutionResult;
    let succeeded = output
        .tx_results
        .iter()
        .filter(|r| matches!(r, Ok(o) if matches!(o.execution_result, ExecutionResult::Success(_))))
        .count();
    assert_eq!(
        succeeded, m_total,
        "expected all {m_total} transfers to succeed, got {succeeded}"
    );
}

/// Times a **single block** of `M` native-token transfers in sequencer mode
/// (no ecrecover, noDA, forward-only). Warms caches/allocator, then times
/// individual `execute_block` calls — the tx `clone()` is done *outside* the
/// timed region, so the measurement is parse + execute + finalize only.
/// Reports min / median / mean block time, per-tx time, and cycles/tx.
///
/// Run with:
/// ```text
/// cargo test -p bench --release --features forward-flamegraph \
///     native_transfers_single_block -- --ignored --nocapture
/// ```
/// Env: `BENCH_N_ACCOUNTS`, `BENCH_M_TRANSFERS` (total transfers),
/// `BENCH_TIMING_RUNS`, `BENCH_CLOCK_GHZ`.
#[cfg(feature = "forward-flamegraph")]
#[test]
#[ignore = "heavy synthetic benchmark; run explicitly (see fn docs)"]
fn native_transfers_single_block() {
    use std::time::Instant;

    let n_accounts: usize = env_or("BENCH_N_ACCOUNTS", 128usize);
    let m_total: usize = env_or("BENCH_M_TRANSFERS", 2800usize);
    let runs: usize = env_or("BENCH_TIMING_RUNS", 50usize);
    let clock_ghz: f64 = env_or("BENCH_CLOCK_GHZ", DEFAULT_CLOCK_GHZ);

    println!("Building {m_total} transfers across {n_accounts} senders...");
    let (mut tester, txs) = build_native_transfer_bench(n_accounts, m_total);

    // Warm up (untimed) and sanity-check correctness on the first run.
    assert_all_succeeded(&tester.execute_block(txs.clone()), m_total);
    let _ = tester.execute_block(txs.clone());

    // Time individual single-block executions. Clone happens OUTSIDE the timed
    // region so we measure only `execute_block` (parse + execute + finalize).
    let mut block_us: Vec<f64> = Vec::with_capacity(runs);
    let mut last_output = None;
    for _ in 0..runs {
        let txs_i = txs.clone();
        let t0 = Instant::now();
        let out = tester.execute_block(txs_i);
        block_us.push(t0.elapsed().as_secs_f64() * 1e6);
        // Retain the result *after* the timing capture so the timed region is
        // still just `execute_block`; the move into the Option is untimed.
        last_output = Some(out);
    }
    // Correctness check on a *measured* run too (excluded from timing): confirms
    // the blocks we actually timed executed every transfer successfully, not
    // just the warmup. Runs replay the same pre-state, so this is representative.
    assert_all_succeeded(
        &last_output.expect("at least one timed run"),
        m_total,
    );
    block_us.sort_by(|a, b| a.partial_cmp(b).unwrap());

    let min = block_us[0];
    let median = block_us[runs / 2];
    let mean = block_us.iter().sum::<f64>() / runs as f64;
    let per_tx = |b: f64| b / m_total as f64; // µs/tx
    let cyc = |b: f64| per_tx(b) * clock_ghz * 1000.0; // cycles/tx

    println!(
        "Single block: {m_total} transfers ({n_accounts} senders), sequencer mode, noDA, \
         {runs} timed runs @ {clock_ghz} GHz"
    );
    println!(
        "  block   : min {:.3} ms   median {:.3} ms   mean {:.3} ms",
        min / 1000.0,
        median / 1000.0,
        mean / 1000.0
    );
    println!(
        "  per-tx  : min {:.3} µs   median {:.3} µs   mean {:.3} µs",
        per_tx(min),
        per_tx(median),
        per_tx(mean)
    );
    println!(
        "  cycles/tx: min ~{:.0}   median ~{:.0}",
        cyc(min),
        cyc(median)
    );
}
