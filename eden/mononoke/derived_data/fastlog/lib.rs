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
mod fastlog_impl;
mod mapping;
mod ops;
mod thrift {
    pub use mononoke_types_thrift::*;
}

pub use mapping::{
    fetch_parent_root_unodes, ErrorKind, FastlogParent, RootFastlog, RootFastlogMapping,
};
pub use ops::{list_file_history, prefetch_history};
