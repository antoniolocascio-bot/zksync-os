# `bench` — native-transfer forward-run benchmark

Measures the **per-transaction execution time of the ZKsync OS forward
(sequencer) run** on a single block of base-token (native ETH) transfers.

The real sequencer verifies EOA signatures at mempool ingress, so its forward
run skips the per-transaction `ecrecover`. This benchmark reproduces that path:
the rig's forward run uses `BasicBootloaderForwardSimulationConfig`
(`VALIDATE_EOA_SIGNATURE = false`), which still charges `ECRECOVER_NATIVE_COST`
for accounting parity while skipping the recovery compute. The run is
forward-only (no RISC-V simulation) and noDA (Validium), and it replays the same
block against a fixed pre-state, so every timed iteration is identical.

## Run it (single command, from the repository root)

```bash
ZKSYNC_USE_CUDA_STUBS=true BENCH_M_TRANSFERS=4758 cargo test -p bench --release --features forward-flamegraph native_transfers_single_block -- --ignored --nocapture
```

`ZKSYNC_USE_CUDA_STUBS=true` lets the airbender-host dependency build on hosts
without a CUDA toolkit (matches CI). `--features forward-flamegraph` enables the
benchmark and routes through `rig/no_print` so bootloader traces stay out of the
timed region.

## Expected output

```
Building 4758 transfers across 128 senders...
Single block: 4758 transfers (128 senders), sequencer mode, noDA, 50 timed runs @ 2.4 GHz
  block   : min 15.013 ms   median 15.272 ms   mean 15.305 ms
  per-tx  : min 3.155 µs   median 3.210 µs   mean 3.217 µs
  cycles/tx: min ~7573   median ~7703
test result: ok. 1 passed; ...
```

`per-tx median` is the headline number. `cycles/tx` is `per-tx × clock_ghz`, a
clock-independent figure for comparing across hosts (set `BENCH_CLOCK_GHZ` to
your core clock for an accurate value).

## Correctness

Every transfer must succeed for the timing to be meaningful. The benchmark
asserts this on an untimed warmup run **and** on a measured run (after the timing
loop, excluded from the timing). `test result: ok` means all `M` transfers
returned `ExecutionResult::Success`.

## Tunable parameters (environment variables)

| Variable | Default | Meaning |
|---|---|---|
| `BENCH_M_TRANSFERS` | `2800` | Total 1-wei transfers in the block |
| `BENCH_N_ACCOUNTS` | `128` | Funded sender EOAs (round-robin, sequential nonces) |
| `BENCH_TIMING_RUNS` | `50` | Number of timed single-block executions |
| `BENCH_CLOCK_GHZ` | `2.4` | Core clock used to report `cycles/tx` |

## How the timing works

`build_native_transfer_bench` funds `N` senders and builds `M` signed
1-wei transfers to a common, pre-created target. Each timed iteration clones the
transaction list **outside** the `Instant::now()` window, so the measurement
covers only `execute_block` (parse + execute + finalize). Reported figures are
min / median / mean across `BENCH_TIMING_RUNS`.
