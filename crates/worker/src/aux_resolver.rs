//! Auxiliary data resolver for ASM Worker.
//!
//! Resolves auxiliary data requests from subprotocols during pre-processing phase.
//! Fetches Bitcoin transactions and historical manifest hashes with MMR proofs.
//!
//! ### Manifest Hash Resolution
//!
//! Fully implemented with on-demand MMR proof generation:
//! - Uses L1 block heights directly as MMR leaf indices (height-indexed MMR)
//! - Fetches manifest hashes from fast lookup storage
//! - Generates MMR proofs using `AsmDBSled`
//! - Converts `MerkleProofB32` (SSZ type) to [`AsmMerkleProof`] (ASM type)
//!
//! ### Bitcoin Transaction Resolution
//!
//! Fully implemented using Bitcoin client's `getrawtransaction` RPC:
//! - Fetches raw transaction data by txid from Bitcoin node
//! - Requires Bitcoin node with transaction indexing enabled (`txindex=1`)
//! - Returns `WorkerError::BitcoinTxNotFound` if transaction not found

use std::fmt;

use strata_asm_common::{
    AsmMerkleProof, AuxData, AuxRequests, BitcoinTxid, ManifestHashRange, RawBitcoinTx,
    VerifiableManifestHash,
};
use tracing::*;

use crate::{L1BlockProvider, ManifestMmrStore, WorkerResult};

/// Auxiliary data resolver that fetches external data required by subprotocols.
///
/// Resolves two types of auxiliary data:
/// 1. Bitcoin transactions by txid
/// 2. Historical manifest hashes with MMR proofs
///
/// Both resolution types are fully implemented:
/// - Bitcoin transaction fetching via Bitcoin RPC (requires txindex=1)
/// - MMR proof generation using AsmDBSled for on-demand proof generation
///
/// Depends only on the two worker-context concerns it actually touches —
/// [`L1BlockProvider`] (transaction fetch) and [`ManifestMmrStore`] (manifest
/// hashes + proofs) — rather than the full `WorkerContext`.
pub struct AuxDataResolver<'a, C: ?Sized + L1BlockProvider + ManifestMmrStore> {
    /// Worker context for accessing Bitcoin transactions and the MMR database.
    context: &'a C,
    /// Leaf count at which manifest proofs should be generated.
    at_leaf_count: u64,
}

impl<'a, C: ?Sized + L1BlockProvider + ManifestMmrStore> AuxDataResolver<'a, C> {
    /// Creates a new auxiliary data resolver.
    ///
    /// # Arguments
    ///
    /// * `context` - Worker context for Bitcoin transaction access and MMR database
    /// * `at_leaf_count` - MMR leaf count snapshot for proof generation
    pub fn new(context: &'a C, at_leaf_count: u64) -> Self {
        Self {
            context,
            at_leaf_count,
        }
    }

    /// Resolves all auxiliary data requests from subprotocols.
    ///
    /// This is the main entry point that coordinates resolution of both
    /// Bitcoin transactions and manifest hashes.
    ///
    /// # Arguments
    ///
    /// * `requests` - Collection of auxiliary data requests from pre-processing
    ///
    /// # Returns
    ///
    /// Returns `AuxData` containing:
    /// - Raw Bitcoin transaction data for each requested txid
    /// - Manifest hashes with MMR proofs for each requested height range
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Any Bitcoin transaction cannot be fetched
    /// - Any historical manifest hash cannot be resolved
    /// - MMR proof generation fails
    pub fn resolve(&self, requests: &AuxRequests) -> WorkerResult<AuxData> {
        debug!(
            bitcoin_txs = requests.bitcoin_txs().len(),
            manifest_ranges = requests.manifest_hashes().len(),
            "Resolving auxiliary data requests"
        );

        // Resolve Bitcoin transactions
        let bitcoin_txs = self.resolve_bitcoin_txs(requests.bitcoin_txs())?;

        // Resolve manifest hashes with proofs
        let manifest_hashes = self.resolve_manifest_hashes(requests.manifest_hashes())?;

        debug!(
            resolved_txs = bitcoin_txs.len(),
            resolved_manifests = manifest_hashes.len(),
            "Successfully resolved auxiliary data"
        );

        Ok(AuxData::new(manifest_hashes, bitcoin_txs))
    }

    /// Resolves Bitcoin transactions by their txids.
    ///
    /// Fetches raw transaction data from the Bitcoin client for each requested txid
    /// using the `getrawtransaction` RPC. Requires the Bitcoin node to have transaction
    /// indexing enabled (`txindex=1` in bitcoin.conf).
    ///
    /// # Arguments
    ///
    /// * `txids` - List of Bitcoin transaction IDs to fetch
    ///
    /// # Returns
    ///
    /// Vector of raw Bitcoin transaction data in the same order as requested.
    ///
    /// # Errors
    ///
    /// Returns `WorkerError::BitcoinTxNotFound` if any transaction cannot be fetched.
    /// This can happen if:
    /// - The transaction does not exist
    /// - The Bitcoin node does not have txindex enabled
    /// - There's a network or RPC communication error
    fn resolve_bitcoin_txs(&self, txids: &[BitcoinTxid]) -> WorkerResult<Vec<RawBitcoinTx>> {
        if txids.is_empty() {
            return Ok(Vec::new());
        }

        debug!(count = txids.len(), "Resolving Bitcoin transactions");

        txids
            .iter()
            .map(|txid| {
                trace!(?txid, "Fetching Bitcoin transaction");
                self.context.get_bitcoin_tx(&(*txid).into()).map(Into::into)
            })
            .collect()
    }

    /// Resolves historical manifest hashes with MMR proofs.
    ///
    /// For each height range, fetches the stored manifest hashes and generates
    /// MMR proofs using the AsmDBSled implementation. The MMR is height-indexed
    /// (sentinel-prefilled at and before genesis), so L1 block heights are used
    /// directly as MMR leaf indices — no offset translation is needed.
    ///
    /// # Arguments
    ///
    /// * `ranges` - List of L1 block height ranges to resolve manifest hashes for
    ///
    /// # Returns
    ///
    /// Vector of manifest hashes with their MMR proofs.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Any manifest hash cannot be fetched from storage
    /// - MMR proof generation fails
    fn resolve_manifest_hashes(
        &self,
        ranges: &[ManifestHashRange],
    ) -> WorkerResult<Vec<VerifiableManifestHash>> {
        if ranges.is_empty() {
            return Ok(Vec::new());
        }

        debug!(count = ranges.len(), "Resolving manifest hash ranges");

        let mut resolved = Vec::new();

        for range in ranges {
            let start_height = range.start_height();
            let end_height = range.end_height();

            debug!(start_height, end_height, "Resolving manifest hash range");

            // L1 block height == MMR leaf index (height-indexed MMR).
            for mmr_index in start_height..=end_height {
                // Fetch manifest hash from storage
                let hash = self.context.get_manifest_hash(mmr_index)?;

                // Generate MMR proof for this index
                let proof_b32 = self
                    .context
                    .generate_mmr_proof_at(mmr_index, self.at_leaf_count)?;

                // Convert MerkleProofB32 to AsmMerkleProof.
                // Both types contain the same data: index and cohashes.
                let cohashes: Vec<[u8; 32]> = proof_b32.cohashes();
                let index = proof_b32.index();
                let asm_proof = AsmMerkleProof::from_cohashes(cohashes, index);

                resolved.push(VerifiableManifestHash::new(hash, asm_proof));

                trace!(index = mmr_index, "Resolved manifest hash with proof");
            }
        }

        debug!(
            resolved_count = resolved.len(),
            "Successfully resolved manifest hashes with MMR proofs"
        );

        Ok(resolved)
    }
}

impl<'a, C: ?Sized + L1BlockProvider + ManifestMmrStore> fmt::Debug for AuxDataResolver<'a, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuxDataResolver")
            .field("at_leaf_count", &self.at_leaf_count)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    // TODO(STR-3031): Add tests
    // - test_resolve_empty_requests
    // - test_resolve_bitcoin_txs
    // - test_resolve_manifest_hashes
    // - test_bitcoin_tx_not_found
    // - test_invalid_manifest_range
}
