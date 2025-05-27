/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![allow(unexpected_cfgs)]

//! Common types used by sibling crates

pub mod blake3;
pub mod cas;
pub mod errors;
pub mod fetch_cause;
pub mod fetch_context;
pub mod fetch_mode;
mod format;
pub mod hash;
pub mod hgid;
pub mod key;
pub mod mutation;
pub mod node;
pub mod nodeinfo;
pub mod parents;
pub mod path;
mod phase;
pub mod repo;
pub mod serde_with;
pub mod sha;
pub mod tree;
pub mod workingcopy_client;

pub use crate::blake3::Blake3;
pub use crate::cas::CasDigest;
pub use crate::cas::CasDigestType;
pub use crate::cas::CasFetchedStats;
pub use crate::cas::CasPrefetchOutcome;
pub use crate::fetch_context::FetchContext;
pub use crate::format::SerializationFormat;
pub use crate::hgid::HgId;
pub use crate::key::Key;
pub use crate::node::Node;
pub use crate::nodeinfo::NodeInfo;
pub use crate::parents::Parents;
pub use crate::path::PathComponent;
pub use crate::path::PathComponentBuf;
pub use crate::path::RepoPath;
pub use crate::path::RepoPathBuf;
pub use crate::phase::Phase;
pub use crate::sha::Sha1;
pub use crate::sha::Sha256;
pub use crate::tree::FileType;

pub type Id20 = HgId;

#[cfg(any(test, feature = "for-tests"))]
pub mod testutil;
