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
//! This crate procides all the primitives useful for serving log and blame data (mutable and immutable).

pub mod blame;

pub use crate::blame::blame;
pub use crate::blame::blame_with_content;

use blobrepo::AsBlobRepo;
use changeset_fetcher::ChangesetFetcherArc;
use changeset_fetcher::ChangesetFetcherRef;
use changesets::ChangesetsRef;
use mutable_renames::MutableRenamesRef;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use skiplist::SkiplistIndexRef;
use trait_alias::trait_alias;

/// Trait alias for history traversal ops.
///
/// These are the repo attributes that are necessary to do most of the (mutable)
/// history traversal operations.
#[trait_alias]
pub trait Repo = AsBlobRepo
    + ChangesetFetcherArc
    + ChangesetFetcherRef
    + ChangesetsRef
    + MutableRenamesRef
    + RepoBlobstoreArc
    + RepoDerivedDataRef
    + SkiplistIndexRef
    + Send
    + Sync;
