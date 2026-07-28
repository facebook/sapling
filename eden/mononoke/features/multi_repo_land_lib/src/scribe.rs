/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Fire-and-forget scribe logging for a single bookmark update. Errors are
//! logged to scuba only and never surface to the caller.

use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingRef;
use bookmarks::BookmarkKind;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use metaconfig_types::RepoConfigRef;
use mononoke_types::ChangesetId;
use phases::PhasesRef;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use repo_update_logger::BookmarkInfo;
use repo_update_logger::CommitInfo;
use repo_update_logger::find_draft_ancestors;
use repo_update_logger::log_bookmark_operation;
use repo_update_logger::log_new_commits;

/// Log a bookmark update to scribe plus, for Move/Create, its newly-public
/// commits. Both destinations are gated inside the underlying loggers, so this
/// no-ops when logging is unconfigured. `new_target` is the new position (pass
/// `None` for Delete); a `find_draft_ancestors` failure is logged and swallowed.
pub async fn log_scribe_bookmark_update<R>(
    ctx: &CoreContext,
    repo: &R,
    info: &BookmarkInfo,
    new_target: Option<ChangesetId>,
) where
    R: RepoIdentityRef
        + RepoConfigRef
        + BonsaiGitMappingRef
        + PhasesRef
        + BonsaiGlobalrevMappingRef
        + CommitGraphRef
        + RepoBlobstoreRef
        + Sync,
{
    log_bookmark_operation(ctx, repo, info).await;

    if let Some(new_target) = new_target {
        match find_draft_ancestors(ctx, repo, new_target).await {
            Ok(bcss) => {
                let commit_infos: Vec<CommitInfo> =
                    bcss.iter().map(|bcs| CommitInfo::new(bcs, None)).collect();
                log_new_commits(
                    ctx,
                    repo,
                    Some((&info.bookmark_name, BookmarkKind::Publishing)),
                    commit_infos,
                )
                .await;
            }
            Err(e) => {
                ctx.scuba()
                    .clone()
                    .add("repo_name", repo.repo_identity().name())
                    .add("scribe_log_failure", "commits")
                    .add("scribe_log_error", format!("{e:#}"))
                    .log_with_msg("Failed to find draft ancestors for scribe logging", None);
            }
        }
    }
}
