/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Code for operations on blobrepos that are useful but not essential.

#![deny(warnings)]

mod bonsai;
mod changeset;
mod errors;

pub use crate::bonsai::{BonsaiMFVerify, BonsaiMFVerifyDifference, BonsaiMFVerifyResult};
pub use crate::changeset::{visit_changesets, ChangesetVisitor};
pub use crate::errors::ErrorKind;

use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use context::CoreContext;
use futures::{future::ok, Future};
use futures_ext::FutureExt;
use manifest::BonsaiDiffFileChange;
use mercurial_types::HgFileNodeId;
use mononoke_types::{FileChange, MPath};

/// This is a function that's used to generate additional file changes for rebased diamond merges.
/// It's used in a very specific use case - rebasing of a diamond merge and it should be used with
/// care. Primary consumer of this function is pushrebase, and pushrebase code
/// contains detailed explanation of why this function is necessary.
pub fn convert_diff_result_into_file_change_for_diamond_merge(
    ctx: CoreContext,
    repo: &BlobRepo,
    diff_result: BonsaiDiffFileChange<HgFileNodeId>,
) -> impl Future<Item = (MPath, Option<FileChange>), Error = Error> {
    match diff_result {
        BonsaiDiffFileChange::Changed(path, ty, node_id)
        | BonsaiDiffFileChange::ChangedReusedId(path, ty, node_id) => {
            node_id
                .load(ctx, repo.blobstore())
                .from_err()
                .map(move |envelope| {
                    // Note that we intentionally do not set copy-from info,
                    // even if a file has been copied from somewhere.
                    // BonsaiChangeset requires that copy_from cs id points to one of
                    // the parents of the commit, so if we just fetch the latest copy
                    // info for a file change, then it might point to one of the
                    // ancestors of `onto`, but not necessarily to the onto.
                    let file_change =
                        FileChange::new(envelope.content_id(), ty, envelope.content_size(), None);
                    (path, Some(file_change))
                })
                .left_future()
        }
        BonsaiDiffFileChange::Deleted(path) => ok((path, None)).right_future(),
    }
}
