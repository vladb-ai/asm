//! Traits for the chain worker to interface with the underlying system.
//!
//! The worker's dependencies split into four concerns, each backed by a
//! distinct subsystem in production:
//!
//! - [`L1BlockProvider`] — reads L1 data from the Bitcoin node (blocks, txs, network).
//! - [`AnchorStateStore`] — persists and loads [`AsmState`].
//! - [`ManifestMmrStore`] — manifest persistence and the manifest-hash MMR.
//! - [`AuxDataStore`] — per-block [`AuxData`] for prover consumption.
//!
//! [`WorkerContext`] is the umbrella that combines all four. It has a blanket
//! impl, so an implementor just implements the four concern traits and gets
//! `WorkerContext` for free; consumers that only need one concern can depend on
//! the narrower trait instead of the whole context.

use bitcoin::{Block, Network};
use strata_asm_common::{AsmManifest, AuxData};
use strata_btc_types::{BitcoinTxid, RawBitcoinTx};
use strata_identifiers::{Hash, L1BlockCommitment, L1BlockId};
use strata_merkle::MerkleProofB32;

use crate::{AsmState, WorkerResult};

/// Reads L1 data from the backing Bitcoin source.
pub trait L1BlockProvider {
    /// Fetches a Bitcoin [`Block`] at a given height.
    fn get_l1_block(&self, blockid: &L1BlockId) -> WorkerResult<Block>;

    /// Fetches a raw Bitcoin transaction by txid.
    ///
    /// Returns the raw transaction bytes.
    fn get_bitcoin_tx(&self, txid: &BitcoinTxid) -> WorkerResult<RawBitcoinTx>;

    /// A Bitcoin network identifier.
    fn get_network(&self) -> WorkerResult<Network>;
}

/// Persists and loads the ASM anchor state.
pub trait AnchorStateStore {
    /// Fetches the [`AsmState`] given the block id.
    fn get_anchor_state(&self, blockid: &L1BlockCommitment) -> WorkerResult<AsmState>;

    /// Fetches the latest [`AsmState`] - the one that corresponds to the "highest" block.
    fn get_latest_asm_state(&self) -> WorkerResult<Option<(L1BlockCommitment, AsmState)>>;

    /// Puts the [`AsmState`] into DB.
    fn store_anchor_state(&self, blockid: &L1BlockCommitment, state: &AsmState)
    -> WorkerResult<()>;
}

/// Persists L1 manifests and maintains the manifest-hash MMR.
pub trait ManifestMmrStore {
    /// Stores an [`AsmManifest`] to the L1 database.
    ///
    /// This should be called after each STF execution with the produced manifest.
    fn store_l1_manifest(&self, manifest: AsmManifest) -> WorkerResult<()>;

    /// Appends a manifest hash to the MMR database and returns the leaf index.
    ///
    /// This should be called after each STF execution with the manifest hash.
    fn append_manifest_to_mmr(&self, manifest_hash: Hash) -> WorkerResult<u64>;

    /// Generates an MMR inclusion proof for a leaf at a specific MMR size.
    ///
    /// The `at_leaf_count` parameter specifies the number of leaves that existed
    /// in the MMR when the proof should be constructed. This allows callers to
    /// generate proofs against a historical snapshot of the MMR rather than the
    /// current state.
    ///
    /// Returns a Merkle proof that can be used by a verifier to check the leaf's
    /// inclusion against the corresponding MMR root for that snapshot.
    fn generate_mmr_proof_at(&self, index: u64, at_leaf_count: u64)
    -> WorkerResult<MerkleProofB32>;

    /// Retrieves a manifest hash by its MMR leaf index.
    ///
    /// Reads the hash directly from the MMR structure.
    fn get_manifest_hash(&self, index: u64) -> WorkerResult<Option<Hash>>;
}

/// Persists and loads per-block auxiliary data for the prover.
pub trait AuxDataStore {
    /// Stores [`AuxData`] for a given L1 block.
    ///
    /// This should be called after each STF execution with the auxiliary data
    /// used during the transition, so the prover can use it as input.
    fn store_aux_data(&self, blockid: &L1BlockCommitment, data: &AuxData) -> WorkerResult<()>;

    /// Retrieves [`AuxData`] for a given L1 block.
    fn get_aux_data(&self, blockid: &L1BlockCommitment) -> WorkerResult<Option<AuxData>>;
}

/// Context trait for a worker to interact with the database and Bitcoin Client.
///
/// Umbrella over the four concern traits ([`L1BlockProvider`],
/// [`AnchorStateStore`], [`ManifestMmrStore`], [`AuxDataStore`]). The blanket
/// impl means any type that implements all four automatically implements
/// `WorkerContext`, so implementors never name it directly.
pub trait WorkerContext:
    L1BlockProvider + AnchorStateStore + ManifestMmrStore + AuxDataStore
{
}

impl<T> WorkerContext for T where
    T: L1BlockProvider + AnchorStateStore + ManifestMmrStore + AuxDataStore
{
}
