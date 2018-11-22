use chrono::DateTime;
use chrono::Utc;
use openssl::{
    error::ErrorStack, hash::MessageDigest, pkey::PKeyRef, sign::Signer, sign::Verifier,
};
use super::error::Error;

/// Ideally git or other VCS tool would be used, but they all aim to *prevent* the loss of history.
/// This does NOT work for SBCs where space is limited, so the wheel needs to be re-invented on
/// this one...

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone)]
struct SquashFs(Vec<u8>);

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone)]
enum Action {
    /// Make a new transaction that could fail.
    Update(Vec<Command>, SquashFs),
    /// Revert the last transaction, whether it succeeded or failed.
    /// If another revert is encountered, lrad reverts the revert, restoring the system before that revert.
    /// If a compact is encountered, the node needs to restore all the data that was deleted before the reversion can complete (unless the entire
    /// compact can be skipped over!). The bootstrap nodes (which should essentially be oracles) should probably have this data.
    /// Reverting into a compaction is expensive and discouraged.
    Revert,
    /// Compact n past transactions to save space. Only do this if you absolutely HAVE to, it is not cheap to undo.
    /// The "blockchain" of hashes is maintained, but the data that backs them will be deleted by the nodes that own it in Kademlia.
    /// The compact creates an aggregate delta on the system state, sort of like a VCS release marker.
    /// Nodes may reject the compaction if the system state does not agree with what the authority says, so a compaction can be patched.
    Compact(usize),
    /// Fix a failed update/patch by appending commands onto it.
    /// Fails if the previous update didn't fail or if the patch fails.
    Patch(Vec<Command>, SquashFs),
}

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone)]
struct SignedTransaction {
    signature: Vec<u8>,
    transaction: Vec<u8>,
}

impl SignedTransaction {
    fn verify(&self, verifier: &mut Verifier) -> Result<bool, ErrorStack> {
        verifier.update(self.transaction.as_slice());
        verifier.verify(self.signature.as_slice())
    }
}

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone)]
struct Transaction {
    action: Action,
    datetime: DateTime<Utc>,
}

impl Transaction {
    fn into_signed(self, signer: &mut Signer) -> Result<SignedTransaction, Error> {
        let self_bytes = bincode::serialize(&self)?;
        signer.update(&self_bytes);
        Ok(SignedTransaction {
            signature: signer.sign_to_vec()?,
            transaction: self_bytes,
        })
    }
}

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone)]
enum Command {
    Cd(String),
    Rm(String),
    Mv(String, String),
    Mkdir(String),
    Cp(String, String),
}
