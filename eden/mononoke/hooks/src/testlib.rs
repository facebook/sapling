/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use bookmarks::BookmarksArc;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreArc;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataArc;
use repo_hook_file_content_provider::RepoHookFileContentProvider;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::PushAuthoredBy;

pub trait Repo = RepoBlobstoreRef + RepoBlobstoreArc + BookmarksArc + RepoDerivedDataArc;

/// Test a changeset hook
///
/// Runs the hook against a changeset and returns the outcome.
pub async fn test_changeset_hook(
    ctx: &CoreContext,
    repo: &impl Repo,
    hook: &impl ChangesetHook,
    bookmark_name: &str,
    cs_id: ChangesetId,
    cross_repo_push_source: CrossRepoPushSource,
    push_authored_by: PushAuthoredBy,
) -> Result<HookExecution> {
    let bcs = cs_id.load(ctx, repo.repo_blobstore()).await?;
    let bookmark = BookmarkKey::new(bookmark_name)?;
    let content_provider = RepoHookFileContentProvider::new(repo);
    hook.run(
        ctx,
        &bookmark,
        &bcs,
        &content_provider,
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
    repo: &impl Repo,
    hook: &impl FileHook,
    cs_id: ChangesetId,
    cross_repo_push_source: CrossRepoPushSource,
    push_authored_by: PushAuthoredBy,
) -> Result<Vec<(NonRootMPath, HookExecution)>> {
    let bcs = cs_id.load(ctx, repo.repo_blobstore()).await?;
    let content_provider = RepoHookFileContentProvider::new(repo);
    let mut results = Vec::new();
    for (path, change) in bcs.file_changes() {
        let outcome = hook
            .run(
                ctx,
                &content_provider,
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
