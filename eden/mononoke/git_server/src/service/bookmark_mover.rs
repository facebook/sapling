/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bonsai_tag_mapping::BonsaiTagMappingRef;
use gix_hash::ObjectId;
use gix_object::Kind;
use import_tools::git_reader::GitReader;
use import_tools::set_bookmark;
use import_tools::BookmarkOperation;
use mononoke_api::repo::RepoContextBuilder;
use mononoke_api::BookmarkKey;
use mononoke_types::ChangesetId;
use repo_authorization::AuthorizationContext;

use super::GitMappingsStore;
use super::GitObjectStore;
use crate::command::RefUpdate;
use crate::model::RepositoryRequestContext;
use crate::service::uploader::peel_tag_target;

/// Struct representing a ref update (create, move, delete) operation
pub struct RefUpdateOperation {
    ref_update: RefUpdate,
    affected_changesets: usize,
}

impl RefUpdateOperation {
    pub fn new(ref_update: RefUpdate, affected_changesets: usize) -> Self {
        Self {
            ref_update,
            affected_changesets,
        }
    }
}

/// Method responsible for creating, moving or deleting a git ref
pub async fn set_ref(
    request_context: Arc<RepositoryRequestContext>,
    mappings_store: Arc<GitMappingsStore>,
    object_store: Arc<GitObjectStore>,
    ref_update_op: RefUpdateOperation,
) -> (RefUpdate, Result<()>) {
    let ref_update = ref_update_op.ref_update.clone();
    // TODO(rajshar): Provide better information about failures instead of just an anyhow::Err
    let result = set_ref_inner(request_context, mappings_store, object_store, ref_update_op).await;
    (ref_update, result)
}

async fn set_ref_inner(
    request_context: Arc<RepositoryRequestContext>,
    mappings_store: Arc<GitMappingsStore>,
    object_store: Arc<GitObjectStore>,
    ref_update_op: RefUpdateOperation,
) -> Result<()> {
    let (ctx, repo, repos) = (
        request_context.ctx.clone(),
        request_context.repo.clone(),
        request_context.mononoke_repos.clone(),
    );
    // Create the repo context which is the pre-requisite for moving bookmarks
    let repo_context = RepoContextBuilder::new(ctx.clone(), repo.clone(), repos)
        .await
        .context("Failure in creating RepoContextBuilder for git push")?
        .with_authorization_context(AuthorizationContext::new(&ctx))
        .build()
        .await
        .context("Failure in creating RepoContext for git push")?;
    // Get the bonsai changeset id of the old and the new git commits
    let old_changeset = get_bonsai(
        &mappings_store,
        &object_store,
        &ref_update_op.ref_update.from,
    )
    .await?;
    let new_changeset =
        get_bonsai(&mappings_store, &object_store, &ref_update_op.ref_update.to).await?;
    // Create the bookmark key by stripping the refs/ prefix from the ref name
    let bookmark_key = BookmarkKey::new(
        ref_update_op
            .ref_update
            .ref_name
            .strip_prefix("refs/")
            .unwrap_or(ref_update_op.ref_update.ref_name.as_str()),
    )?;
    let bookmark_operation =
        BookmarkOperation::new(bookmark_key.clone(), old_changeset, new_changeset)?;
    // Flag for client side expectation of allow non fast forward updates. Git clients by default
    // prevent users from pushing non-ffwd updates. If the request reaches the server, then that
    // means the client has explicitly requested for a non-ffwd update and the final result will be
    // governed by the server side config (ofcourse subject to bypass)
    let allow_non_fast_forward = true;
    // Actually perform the ref update
    set_bookmark(
        &ctx,
        &repo_context,
        &bookmark_operation,
        Some(request_context.pushvars.as_ref()),
        allow_non_fast_forward,
        Some(ref_update_op.affected_changesets),
    )
    .await?;
    // If the bookmark is a tag and the operation is a delete, then we need to remove the tag entry
    // from bonsai_tag_mapping table in addition to removing the bookmark entry from bookmarks table
    if bookmark_key.is_tag() && bookmark_operation.is_delete() {
        repo.inner_repo()
            .bonsai_tag_mapping()
            .delete_mappings_by_name(vec![bookmark_key.name().to_string()])
            .await?;
    }
    Ok(())
}

async fn get_bonsai(
    mappings_store: &GitMappingsStore,
    object_store: &GitObjectStore,
    git_oid: &ObjectId,
) -> Result<Option<ChangesetId>> {
    match mappings_store.get_bonsai(git_oid).await {
        result @ Ok(_) => result,
        err => match object_store.get_object(git_oid.as_ref()).await {
            Ok(obj_content) => {
                if let Some(tag) = obj_content.parsed.as_tag() {
                    let (oid, kind) = peel_tag_target(tag, object_store).await?;
                    if kind == Kind::Commit {
                        return mappings_store.get_bonsai(&oid).await;
                    }
                }
                anyhow::bail!("*** Refs pointing to tree or blobs is not allowed")
            }
            _ => err,
        },
    }
}
