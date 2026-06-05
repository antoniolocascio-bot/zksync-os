//! PoC end-to-end test for a **foreign static call**: a tx to the reserved
//! `FOREIGN_STATICCALL_ADDRESS` executes a (locally deployed) getter contract in
//! a read-only frame whose `SLOAD`s are served from another chain's state and
//! verified against that chain's committed interop root, reusing the real
//! flat-tree verifier (`FlatStorageCommitment::verify_and_apply_batch`).
//!
//! Wiring:
//! - The foreign chain's storage is a real `InMemoryTree` (flat tree). It is
//!   handed to the oracle builder; the `ForeignRoutingTreeResponder` answers the
//!   chain-tagged value query during execution and routes the index/proof
//!   queries to it at the seal.
//! - The foreign root is made live by emitting `InteropRootAdded` from the
//!   interop-root-storage address (the reporter event hook ingests it). Emit +
//!   call run in one block.

use std::collections::BTreeMap;
use std::sync::Arc;

use rig::alloy::consensus::TxLegacy;
use rig::alloy::primitives::{Address, TxKind};
use rig::basic_system::system_implementation::flat_storage_model::{
    FlatStorageCommitment, TREE_HEIGHT,
};
use rig::chain::TestingOracleFactory;
use rig::forward_system::run::test_impl::{InMemoryPreimageSource, InMemoryTree};
use rig::forward_system::run::{make_oracle_for_proofs_and_dumps, FriVerifierArtifacts};
use rig::oracle_provider::ZkEENonDeterminismSource;
use rig::ruint::aliases::{B160, U256};
use rig::system_hooks::addresses_constants::{
    FOREIGN_STATICCALL_ADDRESS, L2_INTEROP_ROOT_STORAGE_ADDRESS,
};
use rig::zk_ee::common_structs::da_commitment_scheme::DACommitmentScheme;
use rig::zk_ee::common_structs::derive_flat_storage_key;
use rig::zk_ee::common_structs::ProofData;
use rig::zk_ee::system::metadata::zk_metadata::BlockMetadataFromOracle;
use rig::zk_ee::utils::Bytes32;
use rig::zksync_os_interface::traits::TxListSource;
use rig::zksync_os_interface::types::{ExecutionOutput, ExecutionResult};
use rig::{tx_succeeded, InMemoryFriProofSidecarSource, TestingFramework};
use zksync_os_tests_common::zksync_tx::ZKsyncTxEnvelope;

const FOREIGN_CHAIN_ID: u64 = 271;
const GETTER_SLOT: u8 = 7;
const BLOCK_NUM: u64 = 1;

// keccak256("InteropRootAdded(uint256,uint256,bytes32[])")
const INTEROP_ROOT_ADDED_EVENT_SIG: [u8; 32] = [
    0x6b, 0x45, 0x1b, 0x84, 0x22, 0x63, 0x6e, 0x45, 0xb9, 0x3b, 0xf7, 0xf5, 0x94, 0xfa, 0x2c, 0x17,
    0x69, 0xd0, 0x39, 0x76, 0x6c, 0x42, 0x54, 0xa6, 0xe7, 0xf9, 0xc0, 0xee, 0x17, 0x15, 0xcd, 0xb0,
];

fn b160_to_address(value: B160) -> Address {
    Address::from_slice(&value.to_be_bytes::<20>())
}

fn u256_be(x: u64) -> [u8; 32] {
    U256::from(x).to_be_bytes::<32>()
}

/// Oracle factory that hands per-chain foreign trees to the builder. The routing
/// (chain-tagged value query + SELECT) lives in `ForeignRoutingTreeResponder`,
/// so no extra processor is needed here.
struct ForeignReadOracleFactory {
    foreign_trees: BTreeMap<u64, InMemoryTree<false>>,
}

impl ForeignReadOracleFactory {
    #[allow(clippy::too_many_arguments)]
    fn build(
        &self,
        block_metadata: BlockMetadataFromOracle,
        state_tree: InMemoryTree<false>,
        preimage_source: InMemoryPreimageSource,
        tx_source: TxListSource,
        fri_sidecar: InMemoryFriProofSidecarSource,
        fri_artifacts: Option<Arc<FriVerifierArtifacts>>,
        proof_data: Option<ProofData<FlatStorageCommitment<TREE_HEIGHT>>>,
        da_commitment_scheme: Option<DACommitmentScheme>,
        add_uart: bool,
        use_native_callable_oracles: bool,
    ) -> ZkEENonDeterminismSource {
        make_oracle_for_proofs_and_dumps(
            block_metadata,
            state_tree,
            self.foreign_trees.clone(),
            preimage_source,
            tx_source,
            fri_sidecar,
            fri_artifacts,
            proof_data,
            da_commitment_scheme,
            add_uart,
            use_native_callable_oracles,
        )
    }
}

impl TestingOracleFactory<false> for ForeignReadOracleFactory {
    #[allow(clippy::too_many_arguments)]
    fn create_forward_oracle(
        &self,
        block_metadata: BlockMetadataFromOracle,
        state_tree: InMemoryTree<false>,
        preimage_source: InMemoryPreimageSource,
        tx_source: TxListSource,
        fri_sidecar: InMemoryFriProofSidecarSource,
        fri_artifacts: Option<Arc<FriVerifierArtifacts>>,
        proof_data: Option<ProofData<FlatStorageCommitment<TREE_HEIGHT>>>,
        da_commitment_scheme: Option<DACommitmentScheme>,
        add_uart: bool,
        use_native_callable_oracles: bool,
    ) -> ZkEENonDeterminismSource {
        self.build(
            block_metadata,
            state_tree,
            preimage_source,
            tx_source,
            fri_sidecar,
            fri_artifacts,
            proof_data,
            da_commitment_scheme,
            add_uart,
            use_native_callable_oracles,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn create_proof_oracle(
        &self,
        block_metadata: BlockMetadataFromOracle,
        state_tree: InMemoryTree<false>,
        preimage_source: InMemoryPreimageSource,
        tx_source: TxListSource,
        fri_sidecar: InMemoryFriProofSidecarSource,
        fri_artifacts: Option<Arc<FriVerifierArtifacts>>,
        proof_data: Option<ProofData<FlatStorageCommitment<TREE_HEIGHT>>>,
        da_commitment_scheme: Option<DACommitmentScheme>,
        add_uart: bool,
        use_native_callable_oracles: bool,
    ) -> ZkEENonDeterminismSource {
        self.build(
            block_metadata,
            state_tree,
            preimage_source,
            tx_source,
            fri_sidecar,
            fri_artifacts,
            proof_data,
            da_commitment_scheme,
            add_uart,
            use_native_callable_oracles,
        )
    }
}

/// EVM bytecode that emits `InteropRootAdded(chain_id, block_num, [root])`.
fn build_emitter_code(chain_id: u64, block_num: u64, root: &Bytes32) -> Vec<u8> {
    let mut c = Vec::new();
    let push32 = |out: &mut Vec<u8>, v: &[u8; 32]| {
        out.push(0x7f); // PUSH32
        out.extend_from_slice(v);
    };
    // ABI-encoded `bytes32[] sides` with one element, laid out in memory:
    //   mem[0..32]  = 0x20 (offset)
    //   mem[32..64] = 1    (length)
    //   mem[64..96] = root (sides[0])
    c.extend_from_slice(&[0x60, 0x20, 0x60, 0x00, 0x52]); // PUSH1 0x20 PUSH1 0 MSTORE
    c.extend_from_slice(&[0x60, 0x01, 0x60, 0x20, 0x52]); // PUSH1 1    PUSH1 0x20 MSTORE
    push32(&mut c, root.as_u8_array_ref());
    c.extend_from_slice(&[0x60, 0x40, 0x52]); // PUSH1 0x40 MSTORE
                                              // LOG3: stack (top->bottom) must be offset, size, topic0, topic1, topic2.
    push32(&mut c, &u256_be(block_num)); // topic2
    push32(&mut c, &u256_be(chain_id)); // topic1
    push32(&mut c, &INTEROP_ROOT_ADDED_EVENT_SIG); // topic0
    c.extend_from_slice(&[0x60, 0x60]); // PUSH1 0x60 (size = 96)
    c.extend_from_slice(&[0x60, 0x00]); // PUSH1 0    (offset)
    c.push(0xa3); // LOG3
    c.push(0x00); // STOP
    c
}

#[test]
fn foreign_static_call_reads_foreign_slot() {
    // ----- foreign chain state: getter's slot holds a known value -----
    let getter_b160 = B160::from_limbs([0xABCD, 0, 0]);
    let getter_addr = b160_to_address(getter_b160);

    let mut slot_bytes = [0u8; 32];
    slot_bytes[31] = GETTER_SLOT;
    let slot = Bytes32::from_array(slot_bytes);

    let mut value_bytes = [0u8; 32];
    value_bytes[28..32].copy_from_slice(&0x1234_5678u32.to_be_bytes());
    let foreign_value = Bytes32::from_array(value_bytes);

    let flat_key = derive_flat_storage_key(&getter_b160, &slot);

    // Build the foreign chain's real flat tree with that slot set.
    let mut foreign_tree = InMemoryTree::<false>::empty();
    foreign_tree.cold_storage.insert(flat_key, foreign_value);
    foreign_tree.storage_tree.insert(&flat_key, &foreign_value);
    let foreign_root = *foreign_tree.storage_tree.root();

    let mut foreign_trees = BTreeMap::new();
    foreign_trees.insert(FOREIGN_CHAIN_ID, foreign_tree);
    let factory = ForeignReadOracleFactory { foreign_trees };

    // ----- contracts deployed locally -----
    // getter: return SLOAD(GETTER_SLOT) as 32 bytes
    let getter_code = vec![
        0x60,
        GETTER_SLOT, // PUSH1 slot
        0x54,        // SLOAD
        0x60,
        0x00, // PUSH1 0
        0x52, // MSTORE
        0x60,
        0x20, // PUSH1 32
        0x60,
        0x00, // PUSH1 0
        0xf3, // RETURN
    ];
    // emitter at the interop-root-storage address (so the reporter hook ingests it)
    let emitter_addr = b160_to_address(L2_INTEROP_ROOT_STORAGE_ADDRESS);
    let emitter_code = build_emitter_code(FOREIGN_CHAIN_ID, BLOCK_NUM, &foreign_root);

    let mut tester = TestingFramework::new()
        .with_evm_contract(getter_addr, &getter_code)
        .with_evm_contract(emitter_addr, &emitter_code)
        .with_custom_oracle_factory(factory);

    let emitter_wallet = tester.prefunded_random_signer();
    let caller_wallet = tester.prefunded_random_signer();

    // tx0: emit the interop root so it is live in this block.
    let emit_tx = ZKsyncTxEnvelope::from_eth_tx(
        TxLegacy {
            chain_id: Some(37),
            nonce: 0,
            gas_price: 25_000,
            gas_limit: 2_000_000,
            to: TxKind::Call(emitter_addr),
            value: Default::default(),
            input: Default::default(),
        },
        emitter_wallet,
    );

    // tx1: foreign static call. calldata = chain_id (32) || to (32, right-aligned).
    let mut input = Vec::with_capacity(64);
    input.extend_from_slice(&u256_be(FOREIGN_CHAIN_ID));
    let mut to_word = [0u8; 32];
    to_word[12..32].copy_from_slice(getter_addr.as_slice());
    input.extend_from_slice(&to_word);

    let call_tx = ZKsyncTxEnvelope::from_eth_tx(
        TxLegacy {
            chain_id: Some(37),
            nonce: 0,
            gas_price: 25_000,
            gas_limit: 5_000_000,
            to: TxKind::Call(b160_to_address(FOREIGN_STATICCALL_ADDRESS)),
            value: Default::default(),
            input: input.into(),
        },
        caller_wallet,
    );

    let output = tester.execute_block(vec![emit_tx, call_tx]);

    assert!(
        tx_succeeded(&output, 0),
        "interop-root emit tx must succeed"
    );
    assert!(tx_succeeded(&output, 1), "foreign static call must succeed");

    let result = output.tx_results[1].as_ref().unwrap();
    match &result.execution_result {
        ExecutionResult::Success(ExecutionOutput::Call(data)) => {
            assert_eq!(data.len(), 32, "foreign static call must return 32 bytes");
            assert_eq!(
                data.as_slice(),
                foreign_value.as_u8_array_ref().as_slice(),
                "returned value must equal the foreign chain's slot value"
            );
        }
        other => panic!("expected success with returndata, got: {other:?}"),
    }
}
