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

use crate::BookmarkHook;
use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::HookResult;
use crate::PushAuthoredBy;

/// Removes the declared message identity from a rejection, so a test can
/// compare the rest of the execution with an expected one.
pub fn without_message(mut execution: HookExecution) -> HookExecution {
    if let HookResult::Rejected(info) = &mut execution.result {
        info.message = None;
    }
    execution
}

/// Test a changeset hook
///
/// Runs the hook against a changeset and returns the outcome, without the
/// declared message identity of a rejection.
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
    let execution = hook
        .run(
            ctx,
            &hook_repo,
            &bookmark,
            &bcs,
            cross_repo_push_source,
            push_authored_by,
            None,
        )
        .await?;
    Ok(without_message(execution))
}

/// Test a bookmark hook
///
/// Runs the hook against a bookmark move to the given changeset and returns
/// the outcome, without the declared message identity of a rejection.
pub async fn test_bookmark_hook(
    ctx: &CoreContext,
    repo: &impl HookRepoLike,
    hook: &impl BookmarkHook,
    bookmark_name: &str,
    cs_id: ChangesetId,
    cross_repo_push_source: CrossRepoPushSource,
    push_authored_by: PushAuthoredBy,
) -> Result<HookExecution> {
    let bcs = cs_id.load(ctx, repo.repo_blobstore()).await?;
    let bookmark = BookmarkKey::new(bookmark_name)?;
    let hook_repo = HookRepo::build_from(repo);
    let execution = hook
        .run(
            ctx,
            &hook_repo,
            &bookmark,
            &bcs,
            cross_repo_push_source,
            push_authored_by,
            None,
        )
        .await?;
    Ok(without_message(execution))
}

/// Test a file hook
///
/// Runs the hook against all the file changes in the changeset, and collects
/// the outcomes, without the declared message identity of rejections.
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
        results.push((path.clone(), without_message(outcome)));
    }
    Ok(results)
}
