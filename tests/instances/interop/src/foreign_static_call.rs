//! PoC end-to-end test for a **foreign static call**: a tx to the reserved
//! `FOREIGN_STATICCALL_ADDRESS` executes a getter contract whose code, account,
//! and storage all live on *another* chain. The foreign frame loads them from
//! that chain's tree and the seal verifies the whole read-set (account leaf +
//! slot) against the chain's committed foreign state root, reusing the real
//! `FlatStorageCommitment::verify_and_apply_batch`.
//!
//! The getter is NOT deployed locally — its account+code live only in the foreign
//! tree (its bytecode/account preimages are made decommit-able via `with_preimage`).
//! The foreign state root is a dedicated input served by the host (distinct from
//! interop/message roots).

use std::collections::BTreeMap;
use std::sync::Arc;

use rig::alloy::consensus::TxLegacy;
use rig::alloy::primitives::{Address, TxKind};
use rig::basic_system::system_implementation::flat_storage_model::{
    address_into_special_storage_key, AccountProperties, FlatStorageCommitment,
    ACCOUNT_PROPERTIES_STORAGE_ADDRESS, TREE_HEIGHT,
};
use rig::chain::TestingOracleFactory;
use rig::forward_system::run::test_impl::{InMemoryPreimageSource, InMemoryTree};
use rig::forward_system::run::{make_oracle_for_proofs_and_dumps, FriVerifierArtifacts};
use rig::oracle_provider::ZkEENonDeterminismSource;
use rig::ruint::aliases::{B160, U256};
use rig::system_hooks::addresses_constants::FOREIGN_STATICCALL_ADDRESS;
use rig::utils::set_properties_code;
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

fn b160_to_address(value: B160) -> Address {
    Address::from_slice(&value.to_be_bytes::<20>())
}

fn u256_be(x: u64) -> [u8; 32] {
    U256::from(x).to_be_bytes::<32>()
}

/// Oracle factory: hands the per-chain foreign trees and their committed state
/// roots to the builder; the `ForeignRoutingTreeResponder` answers from them.
struct ForeignReadOracleFactory {
    foreign_trees: BTreeMap<u64, InMemoryTree<false>>,
    foreign_roots: BTreeMap<u64, Bytes32>,
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
            self.foreign_roots.clone(),
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

#[test]
fn foreign_static_call_reads_foreign_slot() {
    let getter_b160 = B160::from_limbs([0xABCD, 0, 0]);
    let getter_addr = b160_to_address(getter_b160);

    let mut slot_bytes = [0u8; 32];
    slot_bytes[31] = GETTER_SLOT;
    let slot = Bytes32::from_array(slot_bytes);

    let mut value_bytes = [0u8; 32];
    value_bytes[28..32].copy_from_slice(&0x1234_5678u32.to_be_bytes());
    let foreign_value = Bytes32::from_array(value_bytes);

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

    // ----- Build the foreign chain's tree: getter ACCOUNT + CODE + STORAGE.
    // The getter exists only on the foreign chain (mirrors `set_evm_bytecode`).
    let mut props = AccountProperties::default();
    let bytecode_and_artifacts = set_properties_code(&mut props, &getter_code);
    let encoding = props.encoding();
    let properties_hash = props.compute_hash();
    let bytecode_hash = props.bytecode_hash;

    let acct_key = address_into_special_storage_key(&getter_b160);
    let acct_flat_key = derive_flat_storage_key(&ACCOUNT_PROPERTIES_STORAGE_ADDRESS, &acct_key);
    let slot_flat_key = derive_flat_storage_key(&getter_b160, &slot);

    let mut foreign_tree = InMemoryTree::<false>::empty();
    foreign_tree
        .cold_storage
        .insert(acct_flat_key, properties_hash);
    foreign_tree
        .storage_tree
        .insert(&acct_flat_key, &properties_hash);
    foreign_tree
        .cold_storage
        .insert(slot_flat_key, foreign_value);
    foreign_tree
        .storage_tree
        .insert(&slot_flat_key, &foreign_value);
    let foreign_root = *foreign_tree.storage_tree.root();

    let mut foreign_trees = BTreeMap::new();
    foreign_trees.insert(FOREIGN_CHAIN_ID, foreign_tree);
    let mut foreign_roots = BTreeMap::new();
    foreign_roots.insert(FOREIGN_CHAIN_ID, foreign_root);
    let factory = ForeignReadOracleFactory {
        foreign_trees,
        foreign_roots,
    };

    // The getter is NOT deployed locally. Its account-encoding + bytecode preimages
    // must be decommit-able (content-addressed) when the foreign frame loads it.
    let mut tester = TestingFramework::new()
        .with_preimage(bytecode_hash, &bytecode_and_artifacts)
        .with_preimage(properties_hash, &encoding)
        .with_custom_oracle_factory(factory);

    let wallet = tester.prefunded_random_signer();

    // calldata: chain_id (32) || to (32, right-aligned)
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
        wallet,
    );

    let output = tester.execute_block(vec![call_tx]);

    assert!(tx_succeeded(&output, 0), "foreign static call must succeed");
    let result = output.tx_results[0].as_ref().unwrap();
    match &result.execution_result {
        ExecutionResult::Success(ExecutionOutput::Call(data)) => {
            assert_eq!(data.len(), 32, "foreign static call must return 32 bytes");
            assert_eq!(
                data.as_slice(),
                foreign_value.as_u8_array_ref().as_slice(),
                "must return the foreign chain's slot value, read by foreign-loaded code"
            );
        }
        other => panic!("expected success with returndata, got: {other:?}"),
    }
}
