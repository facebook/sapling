// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! QuickCheck support for wire packs.

use quickcheck::{Arbitrary, Gen};

use mercurial_types::{Delta, HgNodeHash, MPath, RepoPath, NULL_HASH};

use super::{DataEntry, HistoryEntry, Kind};

#[derive(Clone, Debug)]
pub struct WirePackPartSequence {
    pub kind: Kind,
    pub files: Vec<FileEntries>,
}

impl Arbitrary for WirePackPartSequence {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let size = g.size();

        let kind = if g.gen_weighted_bool(2) {
            Kind::Tree
        } else {
            Kind::File
        };

        let file_count = g.gen_range(0, size);
        let files = (0..file_count)
            .map(|_| FileEntries::arbitrary_params(g, kind))
            .collect();
        Self { kind, files }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        let kind = self.kind;
        Box::new(self.files.shrink().map(move |files| Self { kind, files }))
    }
}

#[derive(Clone, Debug)]
pub struct FileEntries {
    pub filename: RepoPath,
    pub history: Vec<HistoryEntry>,
    pub data: Vec<DataEntry>,
}

impl FileEntries {
    fn arbitrary_params<G: Gen>(g: &mut G, kind: Kind) -> Self {
        let size = g.size();
        let history_len = g.gen_range(0, size);
        let data_len = g.gen_range(0, size);

        let filename = match kind {
            Kind::Tree => {
                // 10% chance for it to be the root
                if g.gen_weighted_bool(10) {
                    RepoPath::root()
                } else {
                    RepoPath::DirectoryPath(MPath::arbitrary(g))
                }
            }
            Kind::File => RepoPath::FilePath(MPath::arbitrary(g)),
        };
        let history = (0..history_len)
            .map(|_| HistoryEntry::arbitrary_kind(g, kind))
            .collect();
        let data = (0..data_len).map(|_| DataEntry::arbitrary(g)).collect();
        Self {
            filename,
            history,
            data,
        }
    }
}

impl Arbitrary for FileEntries {
    fn arbitrary<G: Gen>(_g: &mut G) -> Self {
        // FileEntries depends on the kind of the overall wirepack, so this can't be implemented.
        unimplemented!("use WirePackPartSequence::arbitrary instead")
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        let filename = self.filename.clone();
        let self_history = self.history.clone();
        let self_data = self.data.clone();
        Box::new(
            (self_history, self_data)
                .shrink()
                .map(move |(history, data)| Self {
                    filename: filename.clone(),
                    history,
                    data,
                }),
        )
    }
}

impl HistoryEntry {
    pub fn arbitrary_kind<G: Gen>(g: &mut G, kind: Kind) -> Self {
        let copy_from = match kind {
            Kind::File => {
                // 20% chance of generating copy-from info
                if g.gen_weighted_bool(5) {
                    Some(RepoPath::FilePath(MPath::arbitrary(g)))
                } else {
                    None
                }
            }
            Kind::Tree => None,
        };
        Self {
            node: HgNodeHash::arbitrary(g),
            p1: HgNodeHash::arbitrary(g),
            p2: HgNodeHash::arbitrary(g),
            linknode: HgNodeHash::arbitrary(g),
            copy_from,
        }
    }
}

impl Arbitrary for HistoryEntry {
    fn arbitrary<G: Gen>(_g: &mut G) -> Self {
        // HistoryEntry depends on the kind of the overall wirepack, so this can't be implemented.
        unimplemented!("use WirePackPartSequence::arbitrary instead")
    }

    // Not going to get anything out of shrinking this since MPath is not shrinkable.
}

impl Arbitrary for DataEntry {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        // 20% chance of a fulltext revision
        let (delta_base, delta) = if g.gen_weighted_bool(5) {
            (NULL_HASH, Delta::new_fulltext(Vec::arbitrary(g)))
        } else {
            let mut delta_base = NULL_HASH;
            while delta_base == NULL_HASH {
                delta_base = HgNodeHash::arbitrary(g);
            }
            (delta_base, Delta::arbitrary(g))
        };
        Self {
            node: HgNodeHash::arbitrary(g),
            delta_base,
            delta,
            version: 1,
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        // The delta is the only shrinkable here.
        let node = self.node;
        let delta_base = self.delta_base;
        Box::new(self.delta.shrink().map(move |delta| Self {
            node,
            delta_base,
            delta,
            version: 1,
        }))
    }
}
