/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! QuickCheck support for wire packs.

use quickcheck::empty_shrinker;
use quickcheck::Arbitrary;
use quickcheck::Gen;

use mercurial_types::Delta;
use mercurial_types::HgNodeHash;
use mercurial_types::MPath;
use mercurial_types::RepoPath;
use mercurial_types::NULL_HASH;
use revisionstore_types::Metadata;

use super::DataEntry;
use super::HistoryEntry;
use super::Kind;

#[derive(Clone, Debug)]
pub struct WirePackPartSequence {
    pub kind: Kind,
    pub files: Vec<FileEntries>,
}

impl Arbitrary for WirePackPartSequence {
    fn arbitrary(g: &mut Gen) -> Self {
        let size = g.size();

        let kind = if bool::arbitrary(g) {
            Kind::Tree
        } else {
            Kind::File
        };

        let file_count = usize::arbitrary(g) % size;
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
    fn arbitrary_params(g: &mut Gen, kind: Kind) -> Self {
        let size = g.size();
        let history_len = usize::arbitrary(g) % size;
        let data_len = usize::arbitrary(g) % size;

        let filename = match kind {
            Kind::Tree => {
                // 10% chance for it to be the root
                if u64::arbitrary(g) % 10 == 0 {
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
    fn arbitrary(_g: &mut Gen) -> Self {
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
    pub fn arbitrary_kind(g: &mut Gen, kind: Kind) -> Self {
        let copy_from = match kind {
            Kind::File => {
                // 20% chance of generating copy-from info
                if *g.choose(&[0, 1, 2, 3, 4]).unwrap() == 0 {
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
    fn arbitrary(_g: &mut Gen) -> Self {
        // HistoryEntry depends on the kind of the overall wirepack, so this can't be implemented.
        unimplemented!("use WirePackPartSequence::arbitrary instead")
    }

    // Not going to get anything out of shrinking this since MPath is not shrinkable.
}

impl Arbitrary for DataEntry {
    fn arbitrary(g: &mut Gen) -> Self {
        // 20% chance of a fulltext revision
        let (delta_base, delta) = if *g.choose(&[0, 1, 2, 3, 4]).unwrap() == 0 {
            (NULL_HASH, Delta::new_fulltext(Vec::arbitrary(g)))
        } else {
            let mut delta_base = NULL_HASH;
            while delta_base == NULL_HASH {
                delta_base = HgNodeHash::arbitrary(g);
            }
            (delta_base, Delta::arbitrary(g))
        };

        // 50% chance of having metadata (i.e. being v2)
        let metadata = if bool::arbitrary(g) {
            // 50% chance of flags being present
            let flags = if bool::arbitrary(g) { Some(1) } else { None };
            // 50% chance of size being present
            let size = if bool::arbitrary(g) { Some(2) } else { None };
            Some(Metadata { flags, size })
        } else {
            None
        };

        Self {
            node: HgNodeHash::arbitrary(g),
            delta_base,
            delta,
            metadata,
        }
    }

    fn shrink(&self) -> Box<dyn Iterator<Item = Self>> {
        // The delta is the only shrinkable here. However, we cannot shrink it if we don't have
        // base (this might generate a non-fulltext delta).
        if self.delta_base == NULL_HASH {
            empty_shrinker()
        } else {
            let node = self.node;
            let delta_base = self.delta_base;
            let metadata = self.metadata;
            Box::new(self.delta.shrink().map(move |delta| Self {
                node,
                delta_base,
                delta,
                metadata,
            }))
        }
    }
}
