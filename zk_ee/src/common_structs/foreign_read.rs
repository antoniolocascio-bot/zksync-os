//! PoC: cross-chain foreign storage reads, reusing the real flat-storage model.
//!
//! A foreign static call runs the callee's bytecode in a read-only frame whose
//! account, code, and storage reads are served from another ("foreign") chain.
//! Those reads go into a per-chain storage model and are verified at the storage
//! seal against that chain's committed **foreign state root** by the real
//! `FlatStorageCommitment::verify_and_apply_batch`.
//!
//! The foreign state root is a dedicated input — conceptually distinct from
//! interop (cross-chain message) roots: it is the other chain's *storage tree
//! root*. [`ForeignStateRootQuery`] brings it in per chain; in a full design it
//! is committed to the batch public output and validated by the settlement layer
//! against that chain's actual committed state root.
//!
//! [`SelectForeignChainQuery`] tells the host which chain's tree should answer
//! the flat-tree index/proof/value queries — set around the foreign frame during
//! execution, and per-chain at the seal. `chain_id == 0` resets to local.

use crate::oracle::query_ids::ACCOUNT_AND_STORAGE_SUBSPACE_MASK;
use crate::oracle::simple_oracle_query::SimpleOracleQuery;
use crate::utils::Bytes32;

/// Select the foreign chain whose tree subsequent flat-tree queries target.
/// `chain_id == 0` resets to the local tree. Response is empty.
pub const SELECT_FOREIGN_CHAIN_QUERY_ID: u32 = ACCOUNT_AND_STORAGE_SUBSPACE_MASK | 0x11;

/// Read the committed foreign state root (storage tree root) for a chain.
pub const FOREIGN_STATE_ROOT_QUERY_ID: u32 = ACCOUNT_AND_STORAGE_SUBSPACE_MASK | 0x12;

/// Routes the host's flat-tree responder to a chain's tree (`0` = local).
pub struct SelectForeignChainQuery;
impl SimpleOracleQuery for SelectForeignChainQuery {
    const QUERY_ID: u32 = SELECT_FOREIGN_CHAIN_QUERY_ID;
    type Input = u64;
    type Output = ();
}

/// The committed state root of foreign chain `chain_id` (a dedicated input,
/// distinct from interop/message roots), used to verify that chain's foreign
/// read-set at the seal.
pub struct ForeignStateRootQuery;
impl SimpleOracleQuery for ForeignStateRootQuery {
    const QUERY_ID: u32 = FOREIGN_STATE_ROOT_QUERY_ID;
    type Input = u64;
    type Output = Bytes32;
}
