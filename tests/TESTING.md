# Testing Guide

This document defines the preferred pattern for integration tests under `tests/instances/*`.

## Goals

- Keep tests short, explicit, and deterministic.
- Use one high-level API (`TestingFramework`) instead of low-level chain plumbing.
- Make tests easy to generate and review (including by AI agents).

## Default Pattern

Prefer this structure in each test:

1. Arrange chain state with `TestingFramework` builder methods.
2. Build transactions with `rig::builder::TxBuilder`.
3. Execute a block with `execute_block`.
4. Assert behavior with `rig` assertion macros and explicit value checks.

```rust
use rig::alloy::primitives::{address, U256 as AlloyU256};
use rig::alloy::signers::local::PrivateKeySigner;
use rig::builder::TxBuilder;
use rig::constants::*;
use rig::run_config;
use rig::ruint::aliases::U256;
use rig::TestingFramework;
use rig::assert_tx_success;

fn new_tester() -> TestingFramework<false> {
    TestingFramework::new().with_run_config(run_config::forward_only())
}

#[test]
fn transfer_succeeds() {
    let signer = PrivateKeySigner::random();
    let sender = signer.address();
    let recipient = address!("deadbeef00000000000000000000000000000001");

    let mut tester = new_tester().with_balance(sender, U256::from(DEFAULT_BALANCE));
    let tx = TxBuilder::new()
        .from(signer)
        .to(recipient)
        .value(AlloyU256::from(1_000u64))
        .gas_limit(TRANSFER_GAS_LIMIT)
        .build();

    let output = tester.execute_block(vec![tx]);
    assert_tx_success!(output, 0);
}
```

## What To Use

- `TestingFramework`:
  - `.with_balance(...)`
  - `.with_evm_contract(...)`
  - `.with_storage_slot(...)`
  - `.with_block_context(...)`
  - `.execute_block(...)`
- `TxBuilder`:
  - `.eip1559()` / `.legacy()` / `.eip2930()` / `.l1()` / `.upgrade()`
  - `.from(...)`, `.to(...)`, `.create()`, `.nonce(...)`
  - `.gas_limit(...)`, `.value(...)`, `.calldata(...)`
  - `.max_fee(...)`, `.priority_fee(...)`
- Assertion macros:
  - `assert_tx_success!`, `assert_tx_reverted!`, `assert_tx_failed!`
  - `assert_gas_used_lt!`, `assert_gas_used_gt!`, `assert_gas_used_between!`
  - `assert_nonce!`, `assert_account_balance!`

## Run Config Presets

Use `rig::run_config` helpers instead of building config manually:

- `run_config::forward_only()` for fast local iteration.
- `run_config::full_proof()` for forward + RISC-V simulation.
- `run_config::with_profiler(path)` when profiling is needed.
- `run_config::with_witness_dump(path)` when witness output is needed.

## Rules For New Tests

- Prefer `TestingFramework` over direct `Chain` usage.
- Keep each test focused on one behavior or invariant.
- Use known constants (`rig::constants::*`) instead of ad-hoc magic numbers.
- When checking persistence/isolation, assert exact read values, not only success status.
- If behavior is feature-gated, document the expected semantics in the test.

## Running

```bash
cargo test -p <instance-crate>
cargo test -p <instance-crate> --features rig/no_print
ZKSYNC_RISC_V_RUN=true cargo test -p <instance-crate>
```
