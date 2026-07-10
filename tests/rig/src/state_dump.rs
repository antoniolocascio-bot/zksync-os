//! Env-gated, prover-neutral per-block state dump.
//!
//! When the `ZKOS_STATE_DUMP_DIR` env var is set, every block executed through
//! `Chain::run_inner` (i.e. all `Chain::run_block*` entry points, including the
//! path `evm_tester` reaches via `run_block_no_panic`) writes a JSON bundle
//! `<dir>/dump-<counter>-<blocknumber>.json` that a second-prover (ZiSK/REVM
//! guest) BatchInput reader consumes. The bundle is test-agnostic: all
//! bytecodes are reachable through the preimages of the `pre`/`post` state
//! snapshots. Blocks that fail to execute write no dump.
//!
//! All 32-byte values are lowercase hex WITHOUT a 0x prefix.

use crate::chain::StateDump;
use alloy::hex;
use basic_bootloader::bootloader::block_flow::public_input::ChainStateCommitment;
use basic_bootloader::bootloader::block_header::BlockHeader;
use crypto::MiniDigest;
use ruint::aliases::U256;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use zk_ee::common_structs::da_commitment_scheme::DACommitmentScheme;
use zk_ee::utils::Bytes32;
use zksync_os_interface::types::BlockOutput;

/// ZKsync OS spec of this source tree as understood by the second-prover
/// guest (0 = AtlasV1, 1 = AtlasV2, 2 = AtlasV3). This tree is v0.3.0
/// (AtlasV3) exactly.
const SPEC_ID: u8 = 2;

/// Protocol version minor of this source tree. v0.3.0 is protocol v31:
/// `BatchOutput` leads with `chain_id` and commits to
/// `interop_roots_rolling_hash` and `settlement_layer_chain_id`, and the
/// block header commits to the keccak rolling transactions hash.
const PROTOCOL_VERSION_MINOR: u32 = 31;

/// Process-wide counter making dump file names unique across all blocks (and
/// test threads) of a single test-process run.
static DUMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Directory to write per-block state dumps into, if dumping is enabled.
pub(crate) fn dump_dir() -> Option<PathBuf> {
    std::env::var_os("ZKOS_STATE_DUMP_DIR").map(PathBuf::from)
}

/// Everything captured at the batch-initial-state point of `Chain::run_inner`
/// (the exact state committed via `proof_data`; at this version `run_inner`
/// performs no pre-block state mutations).
pub(crate) struct PreBlockSnapshot {
    pub dir: PathBuf,
    /// EIP-2718 signed tx bytes (RLP variant) or ABI-encoded bytes (L1->L2).
    pub signed_txs: Vec<String>,
    pub pre: StateDump,
    pub root_before: Bytes32,
    pub next_free_slot_before: u64,
    pub block_hashes_before: [U256; 256],
    pub previous_block_number: u64,
    pub last_block_timestamp_before: u64,
    pub chain_id: u64,
}

#[derive(serde::Serialize)]
struct TxDump {
    signed: String,
    gas_used: u64,
}

#[derive(serde::Serialize)]
struct BlockEnvDump {
    number: u64,
    timestamp: u64,
    base_fee: u64,
    gas_limit: u64,
    coinbase: String,
    prev_randao: String,
    gas_used: u64,
}

/// Every field of the native `BlockHeader` struct the STF produced for the
/// block, so the consumer can diff its own header reconstruction field by
/// field. Byte fields are lowercase hex without 0x (`difficulty` as a 32-byte
/// BE word); scalar fields are JSON numbers.
#[derive(serde::Serialize)]
struct NativeHeaderDump {
    parent_hash: String,
    ommers_hash: String,
    beneficiary: String,
    state_root: String,
    transactions_root: String,
    receipts_root: String,
    logs_bloom: String,
    difficulty: String,
    number: u64,
    gas_limit: u64,
    gas_used: u64,
    timestamp: u64,
    extra_data: String,
    mix_hash: String,
    nonce: String,
    base_fee_per_gas: u64,
}

impl NativeHeaderDump {
    fn from_header(header: &BlockHeader) -> Self {
        Self {
            parent_hash: hex::encode(header.parent_hash.as_u8_ref()),
            ommers_hash: hex::encode(header.ommers_hash.as_u8_ref()),
            beneficiary: hex::encode(header.beneficiary.to_be_bytes::<20>()),
            state_root: hex::encode(header.state_root.as_u8_ref()),
            transactions_root: hex::encode(header.transactions_root.as_u8_ref()),
            receipts_root: hex::encode(header.receipts_root.as_u8_ref()),
            logs_bloom: hex::encode(header.logs_bloom),
            difficulty: hex::encode(header.difficulty.to_be_bytes::<32>()),
            number: header.number,
            gas_limit: header.gas_limit,
            gas_used: header.gas_used,
            timestamp: header.timestamp,
            extra_data: hex::encode(header.extra_data.as_slice()),
            mix_hash: hex::encode(header.mix_hash.as_u8_ref()),
            nonce: hex::encode(header.nonce),
            base_fee_per_gas: header.base_fee_per_gas,
        }
    }
}

#[derive(serde::Serialize)]
struct BlockDump {
    chain_id: u64,
    spec_id: u8,
    protocol_version_minor: u32,
    da_commitment_scheme: u8,
    block: BlockEnvDump,
    tree_root_before: String,
    leaf_count_before: u64,
    tree_root_after: String,
    leaf_count_after: u64,
    pre: StateDump,
    post: StateDump,
    txs: Vec<TxDump>,
    pubdata: String,
    block_header_hash: String,
    // The native `BlockHeader` the STF produced, field by field, plus its own
    // keccak hash (must equal `block_header_hash`; kept separate so a consumer
    // can detect a divergence between the sealed hash and the header fields).
    native_header: NativeHeaderDump,
    native_header_hash: String,
    block_hashes_blake_before: String,
    previous_block_hashes: Vec<String>,
    // Authoritative native ground-truth commitments.
    native_state_before: String,
    native_state_after: String,
    // Empty at v0.3.0: `ChainConfig` does not exist at this version and the
    // v31 `BatchPublicInput` has no chain-config field.
    native_chain_config_hash: String,
    // Empty at v0.3.0: the native `BatchOutput` is only materialized inside
    // the proving STF (`post_tx_op_proving_*`); the rig's forward run has no
    // native producer for it, so the consumer computes and self-checks these.
    native_batch_output_hash: String,
    native_batch_public_input: String,
    // Chain-config inputs (schema compatibility; no ChainConfig at v0.3.0).
    chain_config_fri: bool,
    chain_config_max_tx_gas_limit: u64,
}

/// Assemble and write the JSON bundle for one successfully executed block.
pub(crate) fn write_block_dump(
    snapshot: PreBlockSnapshot,
    post: StateDump,
    root_after: Bytes32,
    next_free_slot_after: u64,
    block_output: &BlockOutput,
    native_header: &BlockHeader,
    da_commitment_scheme: DACommitmentScheme,
) {
    let hdr = &block_output.header;
    let block_number = hdr.number;
    let block_hash = hdr.hash();

    // ---- Authoritative native state commitments (blake2s ChainStateCommitment) ----
    // state_before: blake2s over all 256 ring entries (each BE-32).
    let mut hasher_before = crypto::blake2s::Blake2s256::new();
    for h in snapshot.block_hashes_before.iter() {
        hasher_before.update(h.to_be_bytes::<32>());
    }
    let last256_before: [u8; 32] = hasher_before.finalize();
    let state_before = ChainStateCommitment {
        state_root: snapshot.root_before,
        next_free_slot: snapshot.next_free_slot_before,
        block_number: snapshot.previous_block_number,
        last_256_block_hashes_blake: last256_before.into(),
        last_block_timestamp: snapshot.last_block_timestamp_before,
    }
    .hash();

    // state_after: blake2s over ring[1..256] (255 entries) then current block hash.
    let mut hasher_after = crypto::blake2s::Blake2s256::new();
    for h in snapshot.block_hashes_before.iter().skip(1) {
        hasher_after.update(h.to_be_bytes::<32>());
    }
    hasher_after.update(block_hash.as_slice());
    let last256_after: [u8; 32] = hasher_after.finalize();
    let state_after = ChainStateCommitment {
        state_root: root_after,
        next_free_slot: next_free_slot_after,
        block_number,
        last_256_block_hashes_blake: last256_after.into(),
        last_block_timestamp: hdr.timestamp,
    }
    .hash();

    // previous 255 block hashes = ring[1..256].
    let previous_block_hashes: Vec<String> = snapshot
        .block_hashes_before
        .iter()
        .skip(1)
        .map(|h| hex::encode(h.to_be_bytes::<32>()))
        .collect();

    let txs: Vec<TxDump> = snapshot
        .signed_txs
        .into_iter()
        .zip(block_output.tx_results.iter())
        .map(|(signed, result)| TxDump {
            signed,
            gas_used: result.as_ref().map(|output| output.gas_used).unwrap_or(0),
        })
        .collect();

    let dump = BlockDump {
        chain_id: snapshot.chain_id,
        spec_id: SPEC_ID,
        protocol_version_minor: PROTOCOL_VERSION_MINOR,
        da_commitment_scheme: da_commitment_scheme as u8,
        block: BlockEnvDump {
            number: block_number,
            timestamp: hdr.timestamp,
            base_fee: hdr.base_fee_per_gas.unwrap_or(0),
            gas_limit: hdr.gas_limit,
            coinbase: hex::encode(hdr.beneficiary.as_slice()),
            prev_randao: hex::encode(hdr.mix_hash.as_slice()),
            gas_used: hdr.gas_used,
        },
        tree_root_before: hex::encode(snapshot.root_before.as_u8_ref()),
        leaf_count_before: snapshot.next_free_slot_before,
        tree_root_after: hex::encode(root_after.as_u8_ref()),
        leaf_count_after: next_free_slot_after,
        pre: snapshot.pre,
        post,
        txs,
        pubdata: hex::encode(&block_output.pubdata),
        block_header_hash: hex::encode(block_hash.as_slice()),
        native_header: NativeHeaderDump::from_header(native_header),
        native_header_hash: hex::encode(native_header.hash()),
        block_hashes_blake_before: hex::encode(last256_before),
        previous_block_hashes,
        native_state_before: hex::encode(state_before),
        native_state_after: hex::encode(state_after),
        native_chain_config_hash: String::new(),
        native_batch_output_hash: String::new(),
        native_batch_public_input: String::new(),
        chain_config_fri: false,
        chain_config_max_tx_gas_limit: 0,
    };

    let counter = DUMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::fs::create_dir_all(&snapshot.dir).expect("state dump: create ZKOS_STATE_DUMP_DIR");
    let path = snapshot
        .dir
        .join(format!("dump-{counter:06}-{block_number}.json"));
    let json = serde_json::to_string(&dump).expect("state dump: serialize block dump");
    std::fs::write(&path, json).expect("state dump: write block dump");
    log::info!("state dump written to {}", path.display());
}
