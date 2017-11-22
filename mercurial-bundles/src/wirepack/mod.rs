// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Wire packs. The format is currently undocumented.

use mercurial_types::{Delta, NodeHash, RepoPath};

pub mod unpacker;

/// What sort of wirepack this is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    /// A wire pack representing tree manifests.
    Tree,
    /// A wire pack representing file contents.
    File,
}

/// An atomic part returned from the wirepack.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Part {
    History(RepoPath, HistoryEntry),
    Data(RepoPath, DataEntry),
    End,
}

impl Part {
    #[cfg(test)]
    pub(crate) fn unwrap_history(self) -> (RepoPath, HistoryEntry) {
        match self {
            Part::History(path, entry) => (path, entry),
            other => panic!("expected wirepack part to be History, was {:?}", other),
        }
    }

    #[cfg(test)]
    pub(crate) fn unwrap_data(self) -> (RepoPath, DataEntry) {
        match self {
            Part::Data(path, entry) => (path, entry),
            other => panic!("expected wirepack part to be Data, was {:?}", other),
        }
    }
}

// TODO: move to mercurial-types
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HistoryEntry {
    pub node: NodeHash,
    // TODO: replace with Parents?
    pub p1: NodeHash,
    pub p2: NodeHash,
    pub linknode: NodeHash,
    pub copy_from: Option<RepoPath>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DataEntry {
    pub node: NodeHash,
    pub delta_base: NodeHash,
    pub delta: Delta,
}
