/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Code for operations on blobrepos that are useful but not essential.

#![feature(trait_alias)]

mod bonsai;
mod changeset;
mod errors;

use std::sync::Arc;

use anyhow::Result;
use blobrepo_override::DangerousOverride;
use blobstore::Blobstore;
use blobstore::Loadable;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use manifest::BonsaiDiffFileChange;
use mercurial_types::HgFileNodeId;
use mononoke_types::FileChange;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;

pub use crate::bonsai::BonsaiMFVerify;
pub use crate::bonsai::BonsaiMFVerifyDifference;
pub use crate::bonsai::BonsaiMFVerifyResult;
pub use crate::changeset::visit_changesets;
pub use crate::errors::ErrorKind;

pub trait Repo = BonsaiHgMappingRef
    + CommitGraphRef
    + RepoDerivedDataRef
    + RepoBlobstoreArc
    + DangerousOverride<Arc<dyn Blobstore>>
    + Send
    + Sync
    + Clone
    + 'static;

/// This is a function that's used to generate additional file changes for rebased diamond merges.
/// It's used in a very specific use case - rebasing of a diamond merge and it should be used with
/// care. Primary consumer of this function is pushrebase, and pushrebase code
/// contains detailed explanation of why this function is necessary.
pub async fn convert_diff_result_into_file_change_for_diamond_merge(
    ctx: &CoreContext,
    repo: &impl RepoBlobstoreRef,
    diff_result: BonsaiDiffFileChange<HgFileNodeId>,
) -> Result<(NonRootMPath, FileChange)> {
    match diff_result {
        BonsaiDiffFileChange::Changed(path, ty, node_id)
        | BonsaiDiffFileChange::ChangedReusedId(path, ty, node_id) => {
            let envelope = node_id.load(ctx, repo.repo_blobstore()).await?;
            // Note that we intentionally do not set copy-from info,
            // even if a file has been copied from somewhere.
            // BonsaiChangeset requires that copy_from cs id points to one of
            // the parents of the commit, so if we just fetch the latest copy
            // info for a file change, then it might point to one of the
            // ancestors of `onto`, but not necessarily to the onto.
            let file_change = FileChange::tracked(
                envelope.content_id(),
                ty,
                envelope.content_size(),
                None,
                mononoke_types::GitLfs::FullContent,
            );
            Ok((path, file_change))
        }
        BonsaiDiffFileChange::Deleted(path) => Ok((path, FileChange::Deletion)),
    }
}
