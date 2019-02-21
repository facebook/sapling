// Copyright Facebook, Inc. 2019.

use serde_derive::{Deserialize, Serialize};

use crate::{key::Key, node::Node, nodeinfo::NodeInfo, parents::Parents};

/// Structure containing the fields corresponding to a HistoryPack's
/// in-memory representation of a file history entry. Useful for
/// adding new entries to a MutableHistoryPack.
#[derive(
    Clone,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize
)]
pub struct PackHistoryEntry {
    pub key: Key,
    pub nodeinfo: NodeInfo,
}

impl PackHistoryEntry {
    /// A LooseHistoryEntry doesn't contain enough information to
    /// construct a PackHistoryEntry because it doesn't contain the
    /// name of file to which the entry refers. (The name is a bytestring
    /// that usually consists of the file's path.) As such, the name
    /// needs to be supplied by the caller in order to convert to
    /// PackHistoryEntry.
    pub fn from_loose(entry: LooseHistoryEntry, name: Vec<u8>) -> Self {
        // If this file was copied, use the original name as the name of
        // the p1 key instead of the current entry's name.
        let p1_name = entry.copyfrom.unwrap_or_else(|| name.clone());
        let parents = match entry.parents {
            Parents::None => Default::default(),
            Parents::One(p1) => {
                let p1 = Key::new(p1_name, p1);
                // If there is no p2, its node hash is null and its name is empty.
                let p2 = Key::default();
                [p1, p2]
            }
            Parents::Two(p1, p2) => {
                let p1 = Key::new(p1_name, p1);
                // If there is a p2, its name must match the current entry's name.
                let p2 = Key::new(name.clone(), p2);
                [p1, p2]
            }
        };

        Self {
            key: Key::new(name, entry.node),
            nodeinfo: NodeInfo {
                parents,
                linknode: entry.linknode,
            },
        }
    }
}

impl From<(LooseHistoryEntry, Vec<u8>)> for PackHistoryEntry {
    fn from((entry, name): (LooseHistoryEntry, Vec<u8>)) -> Self {
        Self::from_loose(entry, name)
    }
}

/// History entry structure containing fields corresponding to
/// a single history record in Mercurial's loose file format.
/// This format contains less information than a PackHistoryEntry
/// (namely, it doesn't contain the name of the file), and has
/// less redundancy, making it more suitable as a compact
/// representation of a history entry for data exchange between
/// the client and server.
#[derive(
    Clone,
    Debug,
    Default,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize
)]
pub struct LooseHistoryEntry {
    pub node: Node,
    pub parents: Parents,
    pub linknode: Node,
    pub copyfrom: Option<Vec<u8>>,
}

impl From<PackHistoryEntry> for LooseHistoryEntry {
    fn from(entry: PackHistoryEntry) -> Self {
        let [p1, p2] = entry.nodeinfo.parents;
        // If the p1's name differs from the entry's name, this means the file
        // was copied, so populate the copyfrom path with the p1 name.
        let copyfrom = if !p1.node.is_null() && !p1.name.is_empty() && p1.name != entry.key.name {
            Some(p1.name)
        } else {
            None
        };

        Self {
            node: entry.key.node,
            parents: Parents::new(p1.node, p2.node),
            linknode: entry.nodeinfo.linknode,
            copyfrom,
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::{quickcheck, Arbitrary};

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for PackHistoryEntry {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let key = Key::arbitrary(g);
        let mut nodeinfo = NodeInfo::arbitrary(g);

        // If this entry has a p2, then that p2's name must match
        // this entry's Key name. In the case of copies, Mercurial
        // always puts the copied from path in the p1 Key name,
        // so p2's name must always match the current entry's name
        // unless p2 is null.
        if !nodeinfo.parents[1].node.is_null() {
            nodeinfo.parents[1].name = key.name.clone();
        }

        // If p1's key contains a null node hash or an empty name,
        // the other field must also be null/empty, since it doesn't
        // make sense to have a file path with a null hash or an empty
        // path with a non-null hash.
        //
        // Likewise, if p1 is null, then p2 must also be null.
        if nodeinfo.parents[0].name.is_empty() || nodeinfo.parents[0].node.is_null() {
            nodeinfo.parents[0] = Key::default();
            nodeinfo.parents[1] = Key::default();
        }

        PackHistoryEntry { key, nodeinfo }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for LooseHistoryEntry {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        // It doesn't make sense to have a non-None copyfrom containing
        // an empty name, so set copyfrom to None in such cases.
        let mut copyfrom = <Option<Vec<u8>>>::arbitrary(g).filter(|name| !name.is_empty());
        let parents = Parents::arbitrary(g);

        // It is not possible to have a copy without a p1, so if there is no p1,
        // set copyfrom to None.
        if parents.p1().is_none() {
            copyfrom = None;
        }

        LooseHistoryEntry {
            node: Node::arbitrary(g),
            parents,
            linknode: Node::arbitrary(g),
            copyfrom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    quickcheck! {
        fn pack_entry_roundtrip(pack: PackHistoryEntry) -> bool {
            let name = pack.key.name.clone();
            let loose = LooseHistoryEntry::from(pack.clone());
            let roundtrip = PackHistoryEntry::from((loose, name));
            pack == roundtrip
        }

        fn loose_entry_roundtrip(loose: LooseHistoryEntry, name: Vec<u8>) -> bool {
            let pack = PackHistoryEntry::from((loose.clone(), name));
            let roundtrip = LooseHistoryEntry::from(pack);
            loose == roundtrip
        }
    }
}
