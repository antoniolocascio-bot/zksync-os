//! DESIGN MOCK for cross-chain foreign state reads, as an extension of the flat
//! storage model. It is self-contained (its own simplified `IoOracle`, not the
//! real `zk_ee::oracle::IOOracle` query trait, and a shrunk Merkle depth in the
//! test tree) — it illustrates the design and compiles/tests on its own; it is
//! NOT wired into the real `IOSubsystem`.
//!
//! Design invariants it preserves:
//!   * the oracle is generic and keyed — local and foreign reads use one path;
//!   * tree consistency is verified in a deferred `seal` pass, never per access;
//!   * foreign access is read-only by construction (there is no foreign write).
//!
//! The only deltas over the local model are a `chain` tag on each logged access
//! and a foreign branch in the delayed verifier.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use crypto::blake2s::Blake2s256;
use crypto::MiniDigest;

pub type ChainId = u64;
pub type Value = [u8; 32];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct FlatKey(pub [u8; 32]);
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hash(pub [u8; 32]);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TreeFormat {
    BlakeBinaryV1,
}
const LOCAL_FORMAT: TreeFormat = TreeFormat::BlakeBinaryV1;

#[derive(Debug, PartialEq)]
pub enum IoError {
    SelfAlias,
    DuplicateSnapshot,
    UnknownChain,
    InconsistentRead,
}

// ---- oracle (generic, keyed; unchanged in spirit from the local model) -----

pub struct MerkleProof {
    pub siblings: Vec<Hash>, // leaf -> root order
}

/// Untrusted prover-supplied data, keyed by `(chain, key)`. Local reads use the
/// same surface with `chain == self`. `value` is consumed during execution;
/// `proof` is consumed only by the deferred verifier.
pub trait IoOracle {
    fn value(&mut self, chain: ChainId, key: FlatKey) -> Value;
    fn proof(&mut self, chain: ChainId, key: FlatKey) -> MerkleProof;
}

// ---- hashing + inclusion ---------------------------------------------------

fn leaf_hash(v: Value) -> Hash {
    let mut h = Blake2s256::new();
    h.update([0u8]); // leaf domain
    h.update(v);
    Hash(h.finalize())
}

fn node_hash(l: Hash, r: Hash) -> Hash {
    let mut h = Blake2s256::new();
    h.update([1u8]); // node domain
    h.update(l.0);
    h.update(r.0);
    Hash(h.finalize())
}

fn bit(k: FlatKey, level: usize) -> u8 {
    (k.0[level / 8] >> (level % 8)) & 1
}

fn recompute_root(fmt: TreeFormat, key: FlatKey, value: Value, proof: &MerkleProof) -> Hash {
    match fmt {
        // The one place format matters; today a single arm, versioned via the
        // snapshot tag so a foreign tree upgrade can add arms without churn.
        TreeFormat::BlakeBinaryV1 => {
            let mut node = leaf_hash(value);
            for (level, sib) in proof.siblings.iter().enumerate() {
                node = if bit(key, level) == 0 {
                    node_hash(node, *sib)
                } else {
                    node_hash(*sib, node)
                };
            }
            node
        }
    }
}

mod tree {
    use super::*;

    fn verify_inclusion<O: IoOracle>(
        o: &mut O,
        root: Hash,
        a: &StorageAccess,
        fmt: TreeFormat,
    ) -> Result<(), IoError> {
        let proof = o.proof(a.chain, a.key); // deferred: proof consumed here, not at read time
        if recompute_root(fmt, a.key, a.value, &proof) == root {
            Ok(())
        } else {
            Err(IoError::InconsistentRead)
        }
    }

    /// EXISTING local verifier: checks every read against `init_root`, applies
    /// writes, returns the new root. Reads are verified for real here; the
    /// write-driven root update is the existing tree machinery (left as the
    /// identity in this mock — no writes are exercised).
    pub fn verify_and_apply<O: IoOracle>(
        o: &mut O,
        init_root: Hash,
        fmt: TreeFormat,
        log: &[StorageAccess],
    ) -> Result<Hash, IoError> {
        for a in log.iter().filter(|a| !a.is_write) {
            verify_inclusion(o, init_root, a, fmt)?;
        }
        Ok(init_root)
    }

    /// NEW, thin: a read-only batch is `verify_and_apply` with no writes against a
    /// root we never advance — same inclusion primitive, same oracle.
    pub fn verify_read_set<O: IoOracle>(
        o: &mut O,
        root: Hash,
        fmt: TreeFormat,
        reads: &[StorageAccess],
    ) -> Result<(), IoError> {
        for a in reads {
            verify_inclusion(o, root, a, fmt)?;
        }
        Ok(())
    }
}

// ---- snapshots (inputs, immutable for the batch) ---------------------------

#[derive(Clone, Copy)]
pub struct ForeignSnapshot {
    pub chain_id: ChainId,
    pub state_root: Hash,
    pub block_number: u64,
    pub block_timestamp: u64,
    pub format: TreeFormat,
}

pub struct ForeignRegistry {
    snapshots: BTreeMap<ChainId, ForeignSnapshot>,
}

impl ForeignRegistry {
    pub fn from_oracle(self_chain: ChainId, header: Vec<ForeignSnapshot>) -> Result<Self, IoError> {
        let mut snapshots = BTreeMap::new();
        for s in header {
            if s.chain_id == self_chain {
                return Err(IoError::SelfAlias);
            }
            if snapshots.insert(s.chain_id, s).is_some() {
                return Err(IoError::DuplicateSnapshot);
            }
        }
        Ok(Self { snapshots })
    }

    // self_chain is never inserted ⇒ a self-read through this path is rejected.
    fn contains(&self, chain: ChainId) -> bool {
        self.snapshots.contains_key(&chain)
    }

    fn iter(&self) -> impl Iterator<Item = &ForeignSnapshot> {
        self.snapshots.values()
    }

    /// Block-env values a foreign frame's CHAINID/NUMBER/TIMESTAMP resolve to.
    pub fn header(&self, chain: ChainId) -> Option<ForeignSnapshot> {
        self.snapshots.get(&chain).copied()
    }

    /// `foreign_commitment` for the public output; settlement recomputes it from
    /// calldata and checks each entry against its finalized-snapshot registry.
    /// BTreeMap order makes it deterministic.
    pub fn commitment(&self) -> Hash {
        let mut h = Blake2s256::new();
        for s in self.snapshots.values() {
            h.update(s.chain_id.to_be_bytes());
            h.update(s.state_root.0);
            h.update(s.block_number.to_be_bytes());
            h.update(s.block_timestamp.to_be_bytes());
            h.update([s.format as u8]);
        }
        Hash(h.finalize())
    }
}

// ---- access log + execution-time model -------------------------------------

#[derive(Clone, Copy)]
pub struct StorageAccess {
    pub chain: ChainId,
    pub key: FlatKey,
    pub value: Value,
    pub is_write: bool,
}

pub struct StorageModel<'a, O: IoOracle> {
    oracle: &'a mut O,
    self_chain: ChainId,
    foreign: &'a ForeignRegistry,
    cache: BTreeMap<(ChainId, FlatKey), Value>, // existing cache, key widened by chain
    log: Vec<StorageAccess>,                    // existing log, entry gains `chain`
}

impl<'a, O: IoOracle> StorageModel<'a, O> {
    pub fn new(oracle: &'a mut O, self_chain: ChainId, foreign: &'a ForeignRegistry) -> Self {
        Self {
            oracle,
            self_chain,
            foreign,
            cache: BTreeMap::new(),
            log: Vec::new(),
        }
    }

    pub fn self_chain(&self) -> ChainId {
        self.self_chain
    }

    // EXISTING: trust the oracle value now, log it, verify later in `seal`.
    pub fn read(&mut self, key: FlatKey) -> Value {
        self.touch(self.self_chain, key)
    }

    // Local only — no chain param exists, which is what makes foreign read-only.
    pub fn write(&mut self, key: FlatKey, v: Value) {
        self.cache.insert((self.self_chain, key), v);
        self.log.push(StorageAccess {
            chain: self.self_chain,
            key,
            value: v,
            is_write: true,
        });
    }

    // NEW: identical path, foreign chain, read-only. Membership check (no hashing)
    // rejects unknown chains and self.
    pub fn read_foreign(&mut self, chain: ChainId, key: FlatKey) -> Result<Value, IoError> {
        if !self.foreign.contains(chain) {
            return Err(IoError::UnknownChain);
        }
        Ok(self.touch(chain, key))
    }

    fn touch(&mut self, chain: ChainId, key: FlatKey) -> Value {
        if let Some(v) = self.cache.get(&(chain, key)) {
            return *v;
        }
        let v = self.oracle.value(chain, key); // SAME mechanism for local and foreign
        self.cache.insert((chain, key), v);
        self.log.push(StorageAccess {
            chain,
            key,
            value: v,
            is_write: false,
        });
        v
    }

    pub fn into_log(self) -> Vec<StorageAccess> {
        self.log
    }
}

// ---- delayed verification (seal) -------------------------------------------

pub struct SealOutput {
    pub new_local_root: Hash,
    pub foreign_commitment: Hash,
}

/// Runs after execution. Local part is the existing verifier; the foreign loop is
/// the whole addition — each chain's read-set checked against its pinned root.
pub fn seal<O: IoOracle>(
    oracle: &mut O,
    log: &[StorageAccess],
    self_chain: ChainId,
    local_init_root: Hash,
    foreign: &ForeignRegistry,
) -> Result<SealOutput, IoError> {
    let (local, ext): (Vec<_>, Vec<_>) = log.iter().copied().partition(|a| a.chain == self_chain);

    // unchanged
    let new_local_root = tree::verify_and_apply(oracle, local_init_root, LOCAL_FORMAT, &local)?;

    // the addition
    for snap in foreign.iter() {
        let reads: Vec<_> = ext
            .iter()
            .copied()
            .filter(|a| a.chain == snap.chain_id)
            .collect();
        tree::verify_read_set(oracle, snap.state_root, snap.format, &reads)?;
    }

    Ok(SealOutput {
        new_local_root,
        foreign_commitment: foreign.commitment(),
    })
}

// ---- in-memory mock + tests ------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const DEPTH: usize = 8; // shrunk for the mock; real tree is 256-deep

    struct MockTree {
        leaves: Vec<Value>,
    }

    impl MockTree {
        fn new() -> Self {
            Self {
                leaves: vec![[0u8; 32]; 1 << DEPTH],
            }
        }
        fn index(key: FlatKey) -> usize {
            key.0[0] as usize // 8-bit leaf index
        }
        fn set(&mut self, key: FlatKey, v: Value) {
            self.leaves[Self::index(key)] = v;
        }
        fn get(&self, key: FlatKey) -> Value {
            self.leaves[Self::index(key)]
        }
        fn levels(&self) -> Vec<Vec<Hash>> {
            let mut levels = Vec::new();
            let mut cur: Vec<Hash> = self.leaves.iter().map(|v| leaf_hash(*v)).collect();
            levels.push(cur.clone());
            while cur.len() > 1 {
                cur = cur.chunks(2).map(|c| node_hash(c[0], c[1])).collect();
                levels.push(cur.clone());
            }
            levels
        }
        fn root(&self) -> Hash {
            *self.levels().last().unwrap().first().unwrap()
        }
        fn proof(&self, key: FlatKey) -> MerkleProof {
            let levels = self.levels();
            let mut idx = Self::index(key);
            let mut siblings = Vec::new();
            for level in levels.iter().take(DEPTH) {
                siblings.push(level[idx ^ 1]);
                idx >>= 1;
            }
            MerkleProof { siblings }
        }
    }

    struct MockOracle {
        trees: BTreeMap<ChainId, MockTree>,
    }
    impl IoOracle for MockOracle {
        fn value(&mut self, chain: ChainId, key: FlatKey) -> Value {
            self.trees.get(&chain).unwrap().get(key)
        }
        fn proof(&mut self, chain: ChainId, key: FlatKey) -> MerkleProof {
            self.trees.get(&chain).unwrap().proof(key)
        }
    }

    fn key(b: u8) -> FlatKey {
        let mut k = [0u8; 32];
        k[0] = b;
        FlatKey(k)
    }
    fn val(b: u8) -> Value {
        let mut v = [0u8; 32];
        v[0] = b;
        v
    }

    const SELF: ChainId = 1;
    const GW: ChainId = 10;

    fn snap(chain: ChainId, root: Hash) -> ForeignSnapshot {
        ForeignSnapshot {
            chain_id: chain,
            state_root: root,
            block_number: 100,
            block_timestamp: 1234,
            format: TreeFormat::BlakeBinaryV1,
        }
    }

    #[test]
    fn local_and_foreign_reads_verify() {
        let mut local = MockTree::new();
        local.set(key(3), val(7));
        let mut gw = MockTree::new();
        gw.set(key(9), val(42));
        let (local_root, gw_root) = (local.root(), gw.root());

        let reg = ForeignRegistry::from_oracle(SELF, vec![snap(GW, gw_root)]).unwrap();
        let mut trees = BTreeMap::new();
        trees.insert(SELF, local);
        trees.insert(GW, gw);
        let mut oracle = MockOracle { trees };

        let log = {
            let mut m = StorageModel::new(&mut oracle, SELF, &reg);
            assert_eq!(m.read(key(3)), val(7));
            assert_eq!(m.read_foreign(GW, key(9)).unwrap(), val(42));
            assert_eq!(m.read_foreign(SELF, key(3)), Err(IoError::UnknownChain)); // self rejected
            m.into_log()
        };

        let out = seal(&mut oracle, &log, SELF, local_root, &reg).unwrap();
        assert_eq!(out.new_local_root, local_root);
        assert_eq!(out.foreign_commitment, reg.commitment());
    }

    #[test]
    fn tampered_foreign_value_fails_seal() {
        let local = MockTree::new();
        let mut gw = MockTree::new();
        gw.set(key(9), val(42));
        let (local_root, gw_root) = (local.root(), gw.root());

        let reg = ForeignRegistry::from_oracle(SELF, vec![snap(GW, gw_root)]).unwrap();
        let mut trees = BTreeMap::new();
        trees.insert(SELF, local);
        trees.insert(GW, gw);
        let mut oracle = MockOracle { trees };

        // a lying prover: logged value 99 != the committed 42 at GW
        let log = [StorageAccess {
            chain: GW,
            key: key(9),
            value: val(99),
            is_write: false,
        }];
        let r = seal(&mut oracle, &log, SELF, local_root, &reg);
        assert!(matches!(r, Err(IoError::InconsistentRead)));
    }

    #[test]
    fn duplicate_and_self_snapshots_rejected() {
        let r = ForeignRegistry::from_oracle(SELF, vec![snap(SELF, Hash([0u8; 32]))]);
        assert!(matches!(r, Err(IoError::SelfAlias)));
        let r = ForeignRegistry::from_oracle(
            SELF,
            vec![snap(GW, Hash([0u8; 32])), snap(GW, Hash([1u8; 32]))],
        );
        assert!(matches!(r, Err(IoError::DuplicateSnapshot)));
    }
}
