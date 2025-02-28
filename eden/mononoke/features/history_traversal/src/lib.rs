/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Utility crate for traversing mutable history
//!
//! While our derived data types crates are enough to serve immutable history for serving
//! the mutable one we need to combine data from multiple sources in more complex algorithm.
//!
//! This crate provides all the primitives useful for serving log and blame data (mutable and immutable).

#![feature(trait_alias)]

mod blame;
mod common;
mod log;

use commit_graph::CommitGraphRef;
pub use log::list_file_history;
pub use log::CsAndPath;
pub use log::FastlogError;
pub use log::FollowMutableRenames;
pub use log::HistoryAcrossDeletions;
pub use log::NextChangeset;
pub use log::TraversalOrder;
pub use log::Visitor;
use mutable_renames::MutableRenamesRef;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;

pub use crate::blame::blame;
pub use crate::blame::blame_with_content;

/// Trait alias for history traversal ops.
///
/// These are the repo attributes that are necessary to do most of the (mutable)
/// history traversal operations.
pub trait Repo = MutableRenamesRef
    + RepoBlobstoreRef
    + RepoBlobstoreArc
    + RepoDerivedDataRef
    + RepoIdentityRef
    + CommitGraphRef
    + Clone
    + Send
    + Sync;
