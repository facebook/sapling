/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use context::CoreContext;
use hook_manager::HookRepo;
use hook_manager::repo::HookRepoLike;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreRef;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::PushAuthoredBy;

/// Test a changeset hook
///
/// Runs the hook against a changeset and returns the outcome.
pub async fn test_changeset_hook(
    ctx: &CoreContext,
    repo: &impl HookRepoLike,
    hook: &impl ChangesetHook,
    bookmark_name: &str,
    cs_id: ChangesetId,
    cross_repo_push_source: CrossRepoPushSource,
    push_authored_by: PushAuthoredBy,
) -> Result<HookExecution> {
    let bcs = cs_id.load(ctx, repo.repo_blobstore()).await?;
    let bookmark = BookmarkKey::new(bookmark_name)?;
    let hook_repo = HookRepo::build_from(repo);
    hook.run(
        ctx,
        &hook_repo,
        &bookmark,
        &bcs,
        cross_repo_push_source,
        push_authored_by,
    )
    .await
}

/// Test a file hook
///
/// Runs the hook against all the file changes in the changeset, and collects
/// the outcomes.
pub async fn test_file_hook(
    ctx: &CoreContext,
    repo: &impl HookRepoLike,
    hook: &impl FileHook,
    cs_id: ChangesetId,
    cross_repo_push_source: CrossRepoPushSource,
    push_authored_by: PushAuthoredBy,
) -> Result<Vec<(NonRootMPath, HookExecution)>> {
    let bcs = cs_id.load(ctx, repo.repo_blobstore()).await?;
    let hook_repo = HookRepo::build_from(repo);
    let mut results = Vec::new();
    for (path, change) in bcs.file_changes() {
        let outcome = hook
            .run(
                ctx,
                &hook_repo,
                change.simplify(),
                path,
                cross_repo_push_source,
                push_authored_by,
            )
            .await?;
        results.push((path.clone(), outcome));
    }
    Ok(results)
}
