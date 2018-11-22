use chrono::DateTime;
use chrono::Utc;

/// Ideally git or other VCS tool would be used, but they all aim to *prevent* the loss of history.
/// This does NOT work for SBCs where space is limited, so the wheel needs to be re-invented on
/// this one...

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone)]
enum Action {
    /// Make a new transaction that could fail.
    Update(Vec<Command>),
    /// Revert the last transaction, whether it succeeded or failed.
    /// If another revert is encountered, lrad reverts the revert, restoring the system before that revert.
    /// If a compact is encountered, the node needs to restore all the data that was deleted before the reversion can complete (unless the entire
    /// compact can be skipped over!). The bootstrap nodes (which should essentially be oracles) should probably have this data.
    /// Reverting into a compaction is expensive and discouraged.
    Revert(Vec<Command>),
    /// Compact n past transactions to save space. Only do this if you absolutely HAVE to, it is not cheap to undo.
    /// The "blockchain" of hashes is maintained, but the data that backs them will be deleted by the nodes that own it in Kademlia.
    /// The compact creates an aggregate delta on the system state, sort of like a VCS release marker.
    /// Nodes may reject the compaction if the system state does not agree with what the authority says, so a compaction can be patched.
    Compact(usize),
    /// Fix a failed transaction by appending commands onto the previous action.
    /// Nodes will reject it if the previous transaction didn't fail.
    Patch(Vec<Command>),
    /// TODO: in this grammar, is this really allowed? Could it just be a revert of the previous?
    Ignore,
}

struct SignedTransaction {
    transaction: Transaction,
    signature: Vec<u8>,
}

struct Transaction {
    action: Action,
    datetime: DateTime<Utc>,
}

#[derive(Eq, PartialEq, Hash, Debug, Serialize, Deserialize, Clone)]
enum Command {
    Cd(String),
    Rm(String),
    Move(String, String),
    Replace(String, String),
    Cp(String, String),
}
