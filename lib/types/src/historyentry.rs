// Copyright Facebook, Inc. 2019.

use serde_derive::{Deserialize, Serialize};

use crate::{hgid::HgId, key::Key, nodeinfo::NodeInfo, parents::Parents, path::RepoPathBuf};

/// Structure containing the fields corresponding to a HistoryPack's
/// in-memory representation of a history entry. Useful for adding
/// new entries to a MutableHistoryPack.
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
pub struct HistoryEntry {
    pub key: Key,
    pub nodeinfo: NodeInfo,
}

impl HistoryEntry {
    /// A WireHistoryEntry doesn't contain enough information to construct
    /// a HistoryEntry because it doesn't contain the path of the file or
    /// directory to which the entry refers. As such, the path needs to be
    /// supplied by the caller in order to perform the conversion.
    pub fn from_wire(entry: WireHistoryEntry, path: RepoPathBuf) -> Self {
        // If this entry represents a copied file, use the original path as the path of
        // the p1 key instead of the current entry's path.
        let p1_path = entry.copyfrom.unwrap_or_else(|| path.clone());
        let parents = match entry.parents {
            Parents::None => Default::default(),
            Parents::One(p1) => {
                let p1 = Key::new(p1_path, p1);
                // If there is no p2, its hgid hash is null and its path is empty.
                let p2 = Key::default();
                [p1, p2]
            }
            Parents::Two(p1, p2) => {
                let p1 = Key::new(p1_path, p1);
                // If there is a p2, its path must match the current entry's path.
                let p2 = Key::new(path.clone(), p2);
                [p1, p2]
            }
        };

        Self {
            key: Key::new(path, entry.node),
            nodeinfo: NodeInfo {
                parents,
                linknode: entry.linknode,
            },
        }
    }
}

impl From<(WireHistoryEntry, RepoPathBuf)> for HistoryEntry {
    fn from((entry, path): (WireHistoryEntry, RepoPathBuf)) -> Self {
        Self::from_wire(entry, path)
    }
}

/// History entry structure containing fields corresponding to
/// a single history record in Mercurial's loose file format.
/// This format contains less information than a HistoryEntry
/// (namely, it doesn't contain the path of the file), and has
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
pub struct WireHistoryEntry {
    pub node: HgId,
    pub parents: Parents,
    pub linknode: HgId,
    pub copyfrom: Option<RepoPathBuf>,
}

impl From<HistoryEntry> for WireHistoryEntry {
    fn from(entry: HistoryEntry) -> Self {
        let [p1, p2] = entry.nodeinfo.parents;
        // If the p1's path differs from the entry's path, this means the file
        // was copied, so populate the copyfrom path with the p1 path.
        let copyfrom = if !p1.hgid.is_null() && !p1.path.is_empty() && p1.path != entry.key.path {
            Some(p1.path)
        } else {
            None
        };

        Self {
            node: entry.key.hgid,
            parents: Parents::new(p1.hgid, p2.hgid),
            linknode: entry.nodeinfo.linknode,
            copyfrom,
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for HistoryEntry {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        let key = Key::arbitrary(g);
        let mut nodeinfo = NodeInfo::arbitrary(g);

        // If this entry has a p2, then that p2's path must match
        // this entry's Key path. In the case of copies, Mercurial
        // always puts the copied from path in the p1 Key's path,
        // so p2's path must always match the current entry's path
        // unless p2 is null.
        if !nodeinfo.parents[1].hgid.is_null() {
            nodeinfo.parents[1].path = key.path.clone();
        }

        // If p1's key contains a null hgid hash or an empty path,
        // the other field must also be null/empty, since it doesn't
        // make sense to have a file path with a null hash or an empty
        // path with a non-null hash.
        //
        // Likewise, if p1 is null, then p2 must also be null.
        if nodeinfo.parents[0].path.is_empty() || nodeinfo.parents[0].hgid.is_null() {
            nodeinfo.parents[0] = Key::default();
            nodeinfo.parents[1] = Key::default();
        }

        Self { key, nodeinfo }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for WireHistoryEntry {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        // It doesn't make sense to have a non-None copyfrom containing
        // an empty name, so set copyfrom to None in such cases.
        let mut copyfrom = <Option<RepoPathBuf>>::arbitrary(g).filter(|name| !name.is_empty());
        let parents = Parents::arbitrary(g);

        // It is not possible to have a copy without a p1, so if there is no p1,
        // set copyfrom to None.
        if parents.p1().is_none() {
            copyfrom = None;
        }

        Self {
            node: HgId::arbitrary(g),
            parents,
            linknode: HgId::arbitrary(g),
            copyfrom,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use quickcheck::quickcheck;

    quickcheck! {
        fn history_entry_roundtrip(entry: HistoryEntry) -> bool {
            let path = entry.key.path.clone();
            let wire = WireHistoryEntry::from(entry.clone());
            let roundtrip = HistoryEntry::from((wire, path));
            entry == roundtrip
        }

        fn wire_entry_roundtrip(wire: WireHistoryEntry, path: RepoPathBuf) -> bool {
            let entry = HistoryEntry::from((wire.clone(), path));
            let roundtrip = WireHistoryEntry::from(entry);
            wire == roundtrip
        }
    }
}
