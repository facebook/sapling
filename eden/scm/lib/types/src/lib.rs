/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Common types used by sibling crates

pub mod errors;
pub mod hash;
pub mod hgid;
pub mod key;
pub mod mutation;
pub mod node;
pub mod nodeinfo;
pub mod parents;
pub mod path;
pub mod serde_with;
pub mod sha;

pub use crate::hgid::HgId;
pub use crate::key::Key;
pub use crate::node::Node;
pub use crate::nodeinfo::NodeInfo;
pub use crate::parents::Parents;
pub use crate::path::PathComponent;
pub use crate::path::PathComponentBuf;
pub use crate::path::RepoPath;
pub use crate::path::RepoPathBuf;
pub use crate::sha::Sha256;

pub type Id20 = HgId;

#[cfg(any(test, feature = "for-tests"))]
pub mod testutil;
