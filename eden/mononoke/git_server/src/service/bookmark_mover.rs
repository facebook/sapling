/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bytes::Bytes;
use import_tools::set_bookmark;
use import_tools::BookmarkOperation;
use mononoke_api::repo::RepoContextBuilder;
use mononoke_api::BookmarkKey;
use repo_authorization::AuthorizationContext;

use super::GitMappingsStore;
use crate::command::RefUpdate;
use crate::model::RepositoryRequestContext;

/// Struct representing a ref update (create, move, delete) operation
pub struct RefUpdateOperation {
    ref_update: RefUpdate,
    affected_changesets: usize,
    pushvars: Option<HashMap<String, Bytes>>,
}

impl RefUpdateOperation {
    pub fn new(
        ref_update: RefUpdate,
        affected_changesets: usize,
        pushvars: Option<HashMap<String, Bytes>>,
    ) -> Self {
        Self {
            ref_update,
            affected_changesets,
            pushvars,
        }
    }
}

/// Method responsible for creating, moving or deleting a git ref
pub async fn set_ref(
    request_context: Arc<RepositoryRequestContext>,
    mappings_store: Arc<GitMappingsStore>,
    ref_update_op: RefUpdateOperation,
) -> (RefUpdate, Result<()>) {
    let ref_update = ref_update_op.ref_update.clone();
    // TODO(rajshar): Provide better information about failures instead of just an anyhow::Err
    let result = set_ref_inner(request_context, mappings_store, ref_update_op).await;
    (ref_update, result)
}

async fn set_ref_inner(
    request_context: Arc<RepositoryRequestContext>,
    mappings_store: Arc<GitMappingsStore>,
    ref_update_op: RefUpdateOperation,
) -> Result<()> {
    let (ctx, repo, repos) = (
        request_context.ctx.clone(),
        request_context.repo.clone(),
        request_context.mononoke_repos.clone(),
    );
    // Create the repo context which is the pre-requisite for moving bookmarks
    let repo_context = RepoContextBuilder::new(ctx.clone(), repo, repos)
        .await
        .context("Failure in creating RepoContextBuilder for git push")?
        .with_authorization_context(AuthorizationContext::new(&ctx))
        .build()
        .await
        .context("Failure in creating RepoContext for git push")?;
    // Get the bonsai changeset id of the old and the new git commits
    let old_changeset = mappings_store
        .get_bonsai(&ref_update_op.ref_update.from)
        .await?;
    let new_changeset = mappings_store
        .get_bonsai(&ref_update_op.ref_update.to)
        .await?;
    // Create the bookmark key by stripping the refs/ prefix from the ref name
    let bookmark_key = BookmarkKey::new(
        ref_update_op
            .ref_update
            .ref_name
            .strip_prefix("refs/")
            .unwrap_or(ref_update_op.ref_update.ref_name.as_str()),
    )?;
    let bookmark_operation = BookmarkOperation::new(bookmark_key, old_changeset, new_changeset)?;
    // Actually perform the ref update
    let allow_non_fast_forward = false; // TODO(rajshar): Use this value from BookmarkConfig instead of hardcoding here
    set_bookmark(
        &ctx,
        &repo_context,
        &bookmark_operation,
        ref_update_op.pushvars.as_ref(),
        allow_non_fast_forward,
        Some(ref_update_op.affected_changesets),
    )
    .await
}
