use ssz::Decode;
use ssz_derive::{Decode as DeriveDecode, Encode as DeriveEncode};
use strata_asm_common::TxInputRef;
use strata_crypto::threshold_signature::SignatureSet;
use strata_l1_envelope_fmt::parser::parse_envelope_payload;

use crate::{actions::MultisigAction, errors::AdministrationTxParseError};

/// A signed administration payload containing both the action and its signatures.
///
/// This structure is serialized with SSZ and embedded in the witness envelope.
/// The OP_RETURN only contains the SPS-50 tag (magic bytes, subprotocol ID, tx type).
#[derive(Clone, Debug, Eq, PartialEq, DeriveEncode, DeriveDecode)]
pub struct SignedPayload {
    /// Sequence number used to prevent replay attacks and enforce ordering.
    pub seqno: u64,
    /// The administrative action being proposed
    pub action: MultisigAction,
    /// The set of ECDSA signatures authorizing this action
    pub signatures: SignatureSet,
}

impl SignedPayload {
    /// Creates a new signed payload combining an action with its signatures.
    pub fn new(seqno: u64, action: MultisigAction, signatures: SignatureSet) -> Self {
        Self {
            seqno,
            action,
            signatures,
        }
    }
}

/// Parses a transaction to extract both the multisig action and the signature set.
///
/// This function extracts the signed payload from the taproot leaf script embedded
/// in the transaction's witness data. The payload contains both the administrative
/// action and its authorizing signatures.
///
/// # Arguments
/// * `tx` - A reference to the transaction input to parse
///
/// # Errors
/// Returns `AdministrationTxParseError` if:
/// - The transaction lacks a taproot leaf script in its witness
/// - The envelope payload cannot be parsed
/// - The signed payload cannot be deserialized
// TODO(STR-2366): Update L1Payload to minimize DA footprint
pub fn parse_tx(tx: &TxInputRef<'_>) -> Result<SignedPayload, AdministrationTxParseError> {
    let tx_type = tx.tag().tx_type();

    // Extract the taproot leaf script from the first input's witness
    let payload_script = tx.tx().input[0]
        .witness
        .taproot_leaf_script()
        .ok_or(AdministrationTxParseError::MalformedTransaction(tx_type))?
        .script;

    // Parse the envelope payload from the script
    let envelope_payload = parse_envelope_payload(&payload_script.into())?;

    // Deserialize the signed payload (action + signatures) from the envelope
    let signed_payload = SignedPayload::from_ssz_bytes(&envelope_payload)
        .map_err(|_| AdministrationTxParseError::MalformedTransaction(tx_type))?;

    Ok(signed_payload)
}

#[cfg(test)]
mod tests {
    use bitcoin::{Transaction, consensus};
    use strata_asm_proto_txs_test_utils::parse_sps50_tx;

    use crate::parser::parse_tx;

    #[test]
    fn test_parse_tx() {
        let raw_tx = hex::decode("0200000000010174d9b2e9417f91b2012fca8305db5416ac85f21c0948697ce80040d25a9da3ed0200000000fdffffff0300000000000000000a6a08414c504e000000002c030000000000002251204427bcee61c28d378b39d4a669f263253c7d43cae9996fbd0e6f526bf26206ccb533065f00000000160014f9dd9cecb47c40c9ca3174cc1bdd8613a242344302473044022044cdcaddffc7a36c39e097fd3b48030491e9545979b343d0768b3815b86843fd02204b1e0793fe82cad6d438ecc8ff5639b357a29ecff297a96ceabc7a33c1c4426c01210266855f4b4ae94a7c0a0e6ddd15bc811c70b88da092a54d563039320d95fa629e00000000").unwrap();
        let tx: Transaction = consensus::deserialize(&raw_tx).unwrap();
        let input = parse_sps50_tx(&tx);

        let admin_tx = parse_tx(&input);

        dbg!(&tx.compute_txid());
        dbg!(&admin_tx);
    }
}
