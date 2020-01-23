/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

/// This library is used to efficiently store file and directory history.
/// For each unode we store a FastlogBatch - thrift structure that stores latest commits and their
/// parents that modified this file or directory. Commits are stored in BFS order.
/// All FastlogBatches are stored in blobstore.
///
/// Commits also store pointers to their parents, however they are stored as an offset to the
/// commit hash in batch. I.e. if we have two commits A and B and A is an ancestor of B, then
/// batch will look like:
/// B, vec![ParentOffset(1)]
/// A, vec![]
///
/// Note that commits where a file was deleted are not stored in FastlogBatch. It also doesn't
/// store a history across deletions i.e. if a file was added, then deleted then added again in
/// commit A, FastlogBatch in commit A will contain only one entry.
///
/// RootFastlog is a derived data which derives FastlogBatch for each unode
/// that was created or modified in this commit.
use anyhow::Error;
use blobrepo::BlobRepo;
use context::CoreContext;
use futures::Future;
use manifest::Entry;
use mononoke_types::{ChangesetId, FileUnodeId, ManifestUnodeId};
use std::sync::Arc;

mod fastlog_impl;
mod mapping;
mod thrift {
    pub use mononoke_types_thrift::*;
}

use fastlog_impl::{fetch_fastlog_batch_by_unode_id, fetch_flattened};

pub use mapping::{
    fetch_parent_root_unodes, ErrorKind, FastlogParent, RootFastlog, RootFastlogMapping,
};

/// Returns history for a given unode if it exists.
/// This is the public API of this crate i.e. what clients should use if they want to
/// fetch the history
pub fn prefetch_history(
    ctx: CoreContext,
    repo: BlobRepo,
    unode_entry: Entry<ManifestUnodeId, FileUnodeId>,
) -> impl Future<Item = Option<Vec<(ChangesetId, Vec<FastlogParent>)>>, Error = Error> {
    let blobstore = Arc::new(repo.get_blobstore());
    fetch_fastlog_batch_by_unode_id(ctx.clone(), blobstore.clone(), unode_entry).and_then(
        move |maybe_fastlog_batch| {
            maybe_fastlog_batch.map(|fastlog_batch| fetch_flattened(&fastlog_batch, ctx, blobstore))
        },
    )
}
