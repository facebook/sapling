// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Common types used by sibling crates

pub mod api;
pub mod dataentry;
pub mod errors;
pub mod hgid;
pub mod historyentry;
pub mod key;
pub mod node;
pub mod nodeinfo;
pub mod parents;
pub mod path;

pub use crate::dataentry::{DataEntry, Validity};
pub use crate::hgid::HgId;
pub use crate::historyentry::{HistoryEntry, WireHistoryEntry};
pub use crate::key::Key;
pub use crate::node::Node;
pub use crate::nodeinfo::NodeInfo;
pub use crate::parents::Parents;
pub use crate::path::{PathComponent, PathComponentBuf, RepoPath, RepoPathBuf};

#[cfg(any(test, feature = "for-tests"))]
pub mod testutil;
