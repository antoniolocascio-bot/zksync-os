//! PoC: cross-chain foreign storage reads, reusing the real flat-storage tree
//! and its verifier.
//!
//! A contract reads a storage slot of *another* chain (a foreign static call).
//! Foreign reads are **deferred**: during execution the value is pulled from
//! chain C's tree via a chain-tagged value query ([`ForeignValueQuery`]) and
//! logged; at the storage seal we `SELECT` chain C ([`SelectForeignChainQuery`],
//! which routes the existing index/proof queries to C's tree) and run the real
//! `FlatStorageCommitment::verify_and_apply_batch` against C's committed root.
//!
//! The committed root comes from the interop-root storage (already folded into
//! the batch public output), so a foreign read is as trustworthy as the foreign
//! chain's own validity proof.

use crate::oracle::query_ids::ACCOUNT_AND_STORAGE_SUBSPACE_MASK;
use crate::oracle::simple_oracle_query::SimpleOracleQuery;
use crate::oracle::usize_serialization::{UsizeDeserializable, UsizeSerializable};
use crate::storage_types::{InitialStorageSlotData, StorageAddress};
use crate::system::errors::internal::InternalError;
use crate::types_config::EthereumIOTypesConfig;
use crate::utils::exact_size_chain::ExactSizeChain;

/// Value of a storage slot on a foreign chain (chain-tagged read), used during
/// execution. Routed by the host to that chain's tree.
pub const FOREIGN_VALUE_QUERY_ID: u32 = ACCOUNT_AND_STORAGE_SUBSPACE_MASK | 0x10;

/// Select the foreign chain whose tree subsequent index/proof queries target.
/// Used at the seal so `verify_and_apply_batch`'s (fixed-id) tree queries are
/// answered from the selected chain's tree. `chain_id == 0` resets to local.
pub const SELECT_FOREIGN_CHAIN_QUERY_ID: u32 = ACCOUNT_AND_STORAGE_SUBSPACE_MASK | 0x11;

/// `(chain_id, address, key)` request for a foreign storage value.
#[derive(Clone, Copy, Debug)]
pub struct ForeignStorageAddress {
    pub chain_id: u64,
    pub address: StorageAddress<EthereumIOTypesConfig>,
}

impl UsizeSerializable for ForeignStorageAddress {
    const USIZE_LEN: usize = <u64 as UsizeSerializable>::USIZE_LEN
        + <StorageAddress<EthereumIOTypesConfig> as UsizeSerializable>::USIZE_LEN;

    fn iter(&self) -> impl ExactSizeIterator<Item = usize> {
        ExactSizeChain::new(
            UsizeSerializable::iter(&self.chain_id),
            UsizeSerializable::iter(&self.address),
        )
    }
}

impl UsizeDeserializable for ForeignStorageAddress {
    const USIZE_LEN: usize = <Self as UsizeSerializable>::USIZE_LEN;

    fn from_iter(src: &mut impl ExactSizeIterator<Item = usize>) -> Result<Self, InternalError> {
        let chain_id = UsizeDeserializable::from_iter(src)?;
        let address = UsizeDeserializable::from_iter(src)?;
        Ok(Self { chain_id, address })
    }
}

/// Execution-time foreign read: value of `address.key` on chain `chain_id`.
pub struct ForeignValueQuery;
impl SimpleOracleQuery for ForeignValueQuery {
    const QUERY_ID: u32 = FOREIGN_VALUE_QUERY_ID;
    type Input = ForeignStorageAddress;
    type Output = InitialStorageSlotData<EthereumIOTypesConfig>;
}

/// Seal-time chain selector: routes the existing index/proof tree queries to the
/// chosen chain's tree (`0` = local). Response is empty.
pub struct SelectForeignChainQuery;
impl SimpleOracleQuery for SelectForeignChainQuery {
    const QUERY_ID: u32 = SELECT_FOREIGN_CHAIN_QUERY_ID;
    type Input = u64;
    type Output = ();
}
