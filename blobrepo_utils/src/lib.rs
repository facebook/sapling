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

use blobrepo::BlobRepo;
use bonsai_utils::BonsaiDiffResult;
use context::CoreContext;
use failure::Error;
use futures::{future::ok, Future};
use futures_ext::FutureExt;
use mononoke_types::{FileChange, MPath};

/// This is a function that's used to generate additional file changes for rebased diamond merges.
/// It's used in a very specific use case - rebasing of a diamond merge and it should be used with
/// care. Primary consumer of this function is pushrebase, and pushrebase code
/// contains detailed explanation of why this function is necessary.
pub fn convert_diff_result_into_file_change_for_diamond_merge(
    ctx: CoreContext,
    repo: &BlobRepo,
    diff_result: BonsaiDiffResult,
) -> impl Future<Item = (MPath, Option<FileChange>), Error = Error> {
    match diff_result {
        BonsaiDiffResult::Changed(path, ty, node_id)
        | BonsaiDiffResult::ChangedReusedId(path, ty, node_id) => {
            let content_id_fut = repo.get_file_content_id(ctx.clone(), node_id);
            let file_size_fut = repo.get_file_size(ctx.clone(), node_id);

            content_id_fut
                .join(file_size_fut)
                .map(move |(content_id, file_size)| {
                    // Note that we intentionally do not set copy-from info,
                    // even if a file has been copied from somewhere.
                    // BonsaiChangeset requires that copy_from cs id points to one of
                    // the parents of the commit, so if we just fetch the latest copy
                    // info for a file change, then it might point to one of the
                    // ancestors of `onto`, but not necessarily to the onto.
                    let file_change = FileChange::new(content_id, ty, file_size, None);
                    (path, Some(file_change))
                })
                .left_future()
        }
        BonsaiDiffResult::Deleted(path) => ok((path, None)).right_future(),
    }
}
