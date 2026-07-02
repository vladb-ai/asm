//! Merkle inclusion proofs for Bitcoin transactions.
//!
//! Provides [`TxidInclusionProof`] for generating and verifying that a transaction is included
//! in a block by reconstructing the Merkle root from sibling hashes.

use bitcoin::Transaction;
use serde::{Deserialize, Serialize};
use ssz_derive::{Decode, Encode};
use strata_btc_types::TxidExt;
use strata_crypto::hash::sha256d;
use strata_identifiers::Buf32;

use crate::compute_txid;

/// A Merkle inclusion proof for a Bitcoin transaction, consisting of the transaction's position
/// in the block and the sibling hashes at each tree level needed to reconstruct the Merkle root.
#[derive(Clone, Debug, PartialEq, Eq, Encode, Decode, Serialize, Deserialize)]
pub struct TxidInclusionProof {
    /// The 0-based position (index) of the transaction within the block's transaction list
    /// for which this proof is generated.
    position: u32,

    /// The sibling hashes needed to reconstruct the Merkle root when combined with the target
    /// transaction's own ID. These are the Merkle tree nodes at each level that pair with the
    /// current hash (either on the left or the right) to produce the next level of the tree.
    siblings: Vec<Buf32>,
}

impl TxidInclusionProof {
    /// Creates a new inclusion proof from a transaction's position and its Merkle siblings.
    pub fn new(position: u32, siblings: Vec<Buf32>) -> Self {
        Self { position, siblings }
    }

    /// Returns the sibling hashes that form the proof path from the leaf to the root.
    pub fn siblings(&self) -> &[Buf32] {
        &self.siblings
    }

    /// Returns the 0-based index of the transaction within the block.
    pub fn position(&self) -> u32 {
        self.position
    }

    /// Generates a Merkle inclusion proof for the transaction at `idx` within the given
    /// `transactions` list.
    ///
    /// Computes all transaction IDs via [`compute_txid`], then walks the Merkle tree to extract
    /// the sibling hashes needed to reconstruct the root from the target transaction's position.
    ///
    /// Bitcoin's Merkle tree duplicates the last element when a level has an odd number of nodes.
    ///
    /// # Panics
    ///
    /// Panics if `idx` is out of bounds for `transactions`.
    pub fn generate(transactions: &[Transaction], idx: u32) -> Self {
        let mut curr_level: Vec<Buf32> = transactions
            .iter()
            .map(|tx| compute_txid(tx).to_buf32())
            .collect();

        assert!(
            (idx as usize) < curr_level.len(),
            "The transaction index ({idx}) should be within the transactions length ({})",
            curr_level.len()
        );

        let mut curr_index = idx;

        // The proof depth is ceil(log2(n)), pre-allocate accordingly.
        let depth = (usize::BITS - curr_level.len().leading_zeros()) as usize;
        let mut siblings = Vec::with_capacity(depth);

        while curr_level.len() > 1 {
            let len = curr_level.len();
            if !len.is_multiple_of(2) {
                curr_level.push(curr_level[len - 1]);
            }

            let sibling_index = if curr_index.is_multiple_of(2) {
                curr_index + 1
            } else {
                curr_index - 1
            };

            siblings.push(curr_level[sibling_index as usize]);

            // Construct the next level by pairwise hashing.
            curr_level = curr_level
                .chunks(2)
                .map(|pair| {
                    let [a, b] = pair else {
                        panic!("chunk should be a pair");
                    };
                    let mut arr = [0u8; 64];
                    arr[..32].copy_from_slice(a.as_bytes());
                    arr[32..].copy_from_slice(b.as_bytes());
                    sha256d(&arr)
                })
                .collect::<Vec<_>>();
            curr_index >>= 1;
        }

        TxidInclusionProof::new(idx, siblings)
    }

    /// Computes the Merkle root for the given `transaction` by hashing it with each sibling
    /// in sequence, using the proof's stored position to determine left/right ordering at
    /// each tree level.
    pub fn compute_root(&self, transaction: &Transaction) -> Buf32 {
        let mut cur_hash = compute_txid(transaction).to_buf32();

        let mut pos = self.position();
        for sibling in self.siblings() {
            let mut buf = [0u8; 64];
            if pos & 1 == 0 {
                buf[..32].copy_from_slice(cur_hash.as_bytes());
                buf[32..].copy_from_slice(sibling.as_bytes());
            } else {
                buf[..32].copy_from_slice(sibling.as_bytes());
                buf[32..].copy_from_slice(cur_hash.as_bytes());
            }
            cur_hash = sha256d(&buf);
            pos >>= 1;
        }
        cur_hash
    }

    /// Verifies the inclusion proof of the given `transaction` against the provided Merkle `root`.
    pub fn verify(&self, transaction: &Transaction, root: Buf32) -> bool {
        self.compute_root(transaction) == root
    }
}

#[cfg(test)]
mod tests {
    use bitcoin::hashes::Hash;
    use strata_test_utils_btc::BtcMainnetSegment;

    use super::*;

    #[test]
    fn test_l1_tx_proof() {
        let btc_chain = BtcMainnetSegment::load();
        let block = btc_chain.get_block_at(40_321).unwrap();
        let merkle_root: Buf32 = block.header.merkle_root.to_byte_array().into();
        let txs = &block.txdata;

        for (idx, tx) in txs.iter().enumerate() {
            let proof = TxidInclusionProof::generate(txs, idx as u32);
            assert!(proof.verify(tx, merkle_root));
        }
    }

    /// Reproduces the inclusion-proof forgery: `verify` never binds the proof to the tree
    /// structure, so it accepts proofs of the wrong length and out-of-range positions.
    #[test]
    fn test_forged_inclusion_proof_is_accepted() {
        let block = BtcMainnetSegment::load_full_block();
        let merkle_root: Buf32 = block.header.merkle_root.to_byte_array().into();
        let txs = &block.txdata;
        assert!(
            txs.len() > 1,
            "need a multi-transaction block to demonstrate the forgery"
        );

        let coinbase = &txs[0];

        // Forgery 1: a zero-length proof that claims the coinbase's own txid is the Merkle root.
        // With no depth binding, `compute_root` just returns the leaf, so any single hash can be
        // passed off as a whole-block root. This is the primitive behind the 64-byte node/tx
        // second-preimage attack: an internal Merkle node presented as a leaf produces a proof
        // shorter than the true tree depth, which the verifier cannot currently detect.
        let empty_proof = TxidInclusionProof::new(0, vec![]);
        let coinbase_txid = compute_txid(coinbase).to_buf32();
        assert!(
            empty_proof.verify(coinbase, coinbase_txid),
            "BUG: zero-length proof was rejected — the depth-binding fix may already be present"
        );

        // Forgery 2: an out-of-range position verifies against the *real* Merkle root. Only the
        // low `siblings.len()` bits of the position feed left/right ordering, so `position` and
        // `position + 2^depth` compute the same root — yet the latter is not a valid leaf index.
        let valid = TxidInclusionProof::generate(txs, 0);
        let depth = valid.siblings().len() as u32;
        let bogus_position = 1u32 << depth;
        assert!(
            bogus_position >= txs.len() as u32,
            "bogus position should be out of range"
        );
        let forged_position = TxidInclusionProof::new(bogus_position, valid.siblings().to_vec());
        assert!(
            forged_position.verify(coinbase, merkle_root),
            "BUG: out-of-range position was rejected — the position check may already be present"
        );
    }
}
