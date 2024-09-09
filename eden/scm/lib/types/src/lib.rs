/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Common types used by sibling crates

pub mod augmented_tree;
pub mod blake3;
pub mod cas;
pub mod errors;
pub mod fetch_mode;
pub mod hash;
pub mod hgid;
pub mod key;
pub mod mutation;
pub mod node;
pub mod nodeinfo;
pub mod parents;
pub mod path;
pub mod repo;
pub mod serde_with;
pub mod sha;
pub mod tree;
pub mod workingcopy_client;

pub use crate::augmented_tree::AugmentedDirectoryNode;
pub use crate::augmented_tree::AugmentedFileNode;
pub use crate::augmented_tree::AugmentedTree;
pub use crate::augmented_tree::AugmentedTreeEntry;
pub use crate::augmented_tree::AugmentedTreeWithDigest;
pub use crate::blake3::Blake3;
pub use crate::cas::CasDigest;
pub use crate::cas::CasDigestType;
pub use crate::hgid::HgId;
pub use crate::key::Key;
pub use crate::node::Node;
pub use crate::nodeinfo::NodeInfo;
pub use crate::parents::Parents;
pub use crate::path::PathComponent;
pub use crate::path::PathComponentBuf;
pub use crate::path::RepoPath;
pub use crate::path::RepoPathBuf;
pub use crate::sha::Sha1;
pub use crate::sha::Sha256;
pub use crate::tree::FileType;

pub type Id20 = HgId;

#[cfg(any(test, feature = "for-tests"))]
pub mod testutil;
