/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use bonsai_tag_mapping::BonsaiTagMappingRef;
use bookmarks::BookmarkPrefix;
use cloned::cloned;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::stream;
use gix_hash::ObjectId;
use gix_object::Kind;
use import_tools::BookmarkOperation;
use import_tools::bookmark::BookmarkOperationErrorReporting;
use import_tools::bookmark::set_bookmarks;
use import_tools::git_reader::GitReader;
use import_tools::set_bookmark;
use mononoke_api::BookmarkKey;
use mononoke_api::MononokeError;
use mononoke_api::Repo;
use mononoke_api::repo::RepoContextBuilder;
use mononoke_api::repo::git::bookmark_exists_with_prefix;
use mononoke_api::repo::git::get_bookmark_state;
use mononoke_api::repo::push_redirector_enabled;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use protocol::bookmarks_provider::wait_for_bookmark_move;
use repo_authorization::AuthorizationContext;
use repo_identity::RepoIdentityRef;

use super::GitMappingsStore;
use super::GitObjectStore;
use crate::command::RefUpdate;
use crate::model::RepositoryRequestContext;
use crate::util::mononoke_source_of_truth;

const HOOK_WIKI_LINK: &str = "https://fburl.com/wiki/mb4wtk1j";
const COMMIT_CLOUD_REF_PREFIX: &str = "refs/commitcloud/upload";

/// Method responsible for creating, moving or deleting a git ref
pub async fn set_ref(
    request_context: Arc<RepositoryRequestContext>,
    mappings_store: Arc<GitMappingsStore>,
    object_store: Arc<GitObjectStore>,
    ref_update: RefUpdate,
) -> (RefUpdate, Result<()>) {
    let result = set_ref_inner(
        request_context,
        mappings_store,
        object_store,
        ref_update.clone(),
    )
    .await;
    (ref_update, result)
}

/// Method responsible for creating, moving or deleting multiple git refs atomically
pub async fn set_refs(
    request_context: Arc<RepositoryRequestContext>,
    mappings_store: Arc<GitMappingsStore>,
    object_store: Arc<GitObjectStore>,
    ref_updates: Vec<RefUpdate>,
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
        .with_authorization_context(AuthorizationContext::new_non_draft(&ctx))
        .build()
        .await
        .context("Failure in creating RepoContext for git push")?;
    let bookmark_operations = stream::iter(
        ref_updates
            .into_iter()
            .filter(|ref_update| !ref_update.is_content()),
    )
    .map(|ref_update| {
        cloned!(mappings_store, object_store);
        async move {
            // Get the bonsai changeset id of the old and the new git commits
            let old_changeset =
                get_bonsai(&mappings_store, &object_store, &ref_update.from).await?;
            let new_changeset = get_bonsai(&mappings_store, &object_store, &ref_update.to).await?;
            // Create the bookmark key by stripping the refs/ prefix from the ref name
            let bookmark_key = BookmarkKey::new(
                ref_update
                    .ref_name
                    .strip_prefix("refs/")
                    .unwrap_or(ref_update.ref_name.as_str()),
            )?;
            BookmarkOperation::new(bookmark_key.clone(), old_changeset, new_changeset)
        }
    })
    .buffer_unordered(20)
    .try_collect::<Vec<_>>()
    .await?;
    // If the bookmark is a tag and the operation is a delete, then we need to remove the tag entry
    // from bonsai_tag_mapping table in addition to removing the bookmark entry from bookmarks table
    let tags_to_delete = bookmark_operations
        .iter()
        .filter_map(|bookmark_operation| {
            if bookmark_operation.bookmark_key.is_tag() && bookmark_operation.is_delete() {
                Some(bookmark_operation.bookmark_key.name().to_string())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    // Do one final check of SoT to ensure that we don't update the bookmark if the repo is locked or sourced in Metagit
    if !mononoke_source_of_truth(&ctx, repo.clone()).await? {
        return Err(anyhow::anyhow!(
            "Mononoke is not the source of truth for this repo"
        ));
    }
    // Flag for client side expectation of allow non fast forward updates. Git clients by default
    // prevent users from pushing non-ffwd updates. If the request reaches the server, then that
    // means the client has explicitly requested for a non-ffwd update and the final result will be
    // governed by the server side config (ofcourse subject to bypass)
    let allow_non_fast_forward = true;
    // Actually perform the ref updates
    let result = set_bookmarks(
        &ctx,
        &repo_context,
        bookmark_operations,
        Some(request_context.pushvars.as_ref()),
        allow_non_fast_forward,
        BookmarkOperationErrorReporting::Plain,
    )
    .await;
    if let Err(e) = result {
        return Err(update_error(&mappings_store, e).await);
    }
    if !tags_to_delete.is_empty() {
        repo.bonsai_tag_mapping()
            .delete_mappings_by_name(&ctx, tags_to_delete)
            .await?;
    }
    Ok(())
}

async fn set_ref_inner(
    request_context: Arc<RepositoryRequestContext>,
    mappings_store: Arc<GitMappingsStore>,
    object_store: Arc<GitObjectStore>,
    ref_update: RefUpdate,
) -> Result<()> {
    let (ctx, repo, repos) = (
        request_context.ctx.clone(),
        request_context.repo.clone(),
        request_context.mononoke_repos.clone(),
    );
    // Check if the push is to a commit cloud ref, if yes return early as we don't need to actually create/move the ref
    if ref_update.ref_name.starts_with(COMMIT_CLOUD_REF_PREFIX) {
        return Ok(());
    }
    // If the ref is a content ref, then we have already processed it in the uploader. There is no
    // bookmark to move in this case
    if ref_update.is_content() {
        // If the ref is a content ref, we do not allow for deletions
        if ref_update.to.is_null() {
            return Err(anyhow::anyhow!(
                "Deletion of refs pointing to trees or blobs (e.g. {}) is not permitted in Mononoke Git for repo {}",
                ref_update.ref_name,
                repo.repo_identity().name()
            ));
        }
        return Ok(());
    }
    // Create the bookmark operation representing the ref update if its allowed
    let bookmark_operation = bookmark_operation(
        &ctx,
        repo.clone(),
        mappings_store.clone(),
        object_store.clone(),
        &ref_update,
    )
    .await?;
    // Create the repo context which is the pre-requisite for moving bookmarks
    let repo_context = RepoContextBuilder::new(ctx.clone(), repo.clone(), repos)
        .await
        .context("Failure in creating RepoContextBuilder for git push")?
        .with_authorization_context(AuthorizationContext::new_non_draft(&ctx))
        .build()
        .await
        .context("Failure in creating RepoContext for git push")?;

    // Do one final check of SoT to ensure that we don't update the bookmark if the repo is locked or sourced in Metagit
    if !mononoke_source_of_truth(&ctx, repo.clone()).await? {
        return Err(anyhow::anyhow!(
            "Mononoke is not the source of truth for this repo"
        ));
    }

    // Flag for client side expectation of allow non fast forward updates. Git clients by default
    // prevent users from pushing non-ffwd updates. If the request reaches the server, then that
    // means the client has explicitly requested for a non-ffwd update and the final result will be
    // governed by the server side config (ofcourse subject to bypass)
    let allow_non_fast_forward = true;
    // Actually perform the ref update
    let result = set_bookmark(
        &ctx,
        &repo_context,
        &bookmark_operation,
        Some(request_context.pushvars.as_ref()),
        allow_non_fast_forward,
        BookmarkOperationErrorReporting::Plain,
    )
    .await;
    if let Err(err) = result {
        anyhow::bail!(update_error(&mappings_store, err).await);
    }
    // If the bookmark is a tag and the operation is a delete, then we need to remove the tag entry
    // from bonsai_tag_mapping table in addition to removing the bookmark entry from bookmarks table
    let bookmark_key = &bookmark_operation.bookmark_key;
    if bookmark_key.is_tag() && bookmark_operation.is_delete() {
        repo.bonsai_tag_mapping()
            .delete_mappings_by_name(&ctx, vec![bookmark_key.name().to_string()])
            .await?;
    }
    // If requested, let's wait for the bookmark move to get reflected in WBC
    if request_context.pushvars.wait_for_wbc_update() {
        wait_for_bookmark_move(
            &ctx,
            &repo,
            bookmark_key,
            bookmark_operation.operation_type.old_changeset(),
        )
        .await?;
    }
    Ok(())
}

async fn bookmark_operation(
    ctx: &CoreContext,
    repo: Arc<Repo>,
    mappings_store: Arc<GitMappingsStore>,
    object_store: Arc<GitObjectStore>,
    ref_update: &RefUpdate,
) -> Result<BookmarkOperation> {
    // Check if push redirector is enabled, if it is then reject the push
    if push_redirector_enabled(ctx, repo.clone()).await? {
        return Err(anyhow::anyhow!(
            "Pushes to repo {0} are disallowed because its source of truth has been moved. Use `git pushrebase` or make changes directly in the repo where the source of truth was moved.",
            repo.repo_identity().name()
        ));
    }
    // Get the bonsai changeset id of the old and the new git commits
    let old_changeset = get_bonsai(&mappings_store, &object_store, &ref_update.from).await?;
    let new_changeset = get_bonsai(&mappings_store, &object_store, &ref_update.to).await?;
    // Create the bookmark key by stripping the refs/ prefix from the ref name
    let bookmark_key = BookmarkKey::new(
        ref_update
            .ref_name
            .strip_prefix("refs/")
            .unwrap_or(ref_update.ref_name.as_str()),
    )?;
    let bookmark_operation =
        BookmarkOperation::new(bookmark_key.clone(), old_changeset, new_changeset)?;
    if bookmark_operation.is_create() {
        let bookmark_prefix_str = if !bookmark_key.as_str().ends_with("/") {
            format!("{bookmark_key}/")
        } else {
            bookmark_key.to_string()
        };
        let bookmark_prefix = BookmarkPrefix::from_str(bookmark_prefix_str.as_str())?;
        if bookmark_exists_with_prefix(ctx, &repo, &bookmark_prefix).await? {
            // reject push
            return Err(anyhow::anyhow!(
                "Creation of bookmark \"{bookmark_key}\" was blocked because it exists as a path prefix of an existing bookmark",
            ));
        }
        for bookmark_prefix_path in MPath::new(bookmark_prefix_str.as_str())?.into_ancestors() {
            let bookmark_prefix_path =
                BookmarkKey::from_str(std::str::from_utf8(&bookmark_prefix_path.to_vec())?)?;

            let client_correlator = ctx
                .metadata()
                .client_info()
                .and_then(|ci| ci.request_info.as_ref().map(|cri| cri.correlator.as_str()));

            let read_from_replica = justknobs::eval(
                "scm/mononoke:use_maybe_stale_freshness_for_bookmarks",
                client_correlator,
                Some("mononoke_api::repo::git::get_bookmark_state"),
            )
            .unwrap_or(false);

            let freshness = if read_from_replica {
                bookmarks::Freshness::MaybeStale
            } else {
                bookmarks::Freshness::MostRecent
            };

            // Check if the path ancestors of this bookmark already exist as bookmark in the repo
            if get_bookmark_state(ctx, &repo, &bookmark_prefix_path, freshness)
                .await?
                .is_existing()
            {
                return Err(anyhow::anyhow!(
                    "Creation of bookmark \"{bookmark_key}\" was blocked because its path prefix \"{bookmark_prefix_path}\" already exists as a bookmark",
                ));
            }
        }
    }
    Ok(bookmark_operation)
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
                if obj_content.is_tag() {
                    let (kind, oid) = object_store.peel_to_target(*git_oid).await?;
                    if kind == Kind::Commit {
                        return mappings_store.get_bonsai(&oid).await;
                    }
                }
                anyhow::bail!(
                    "Refs pointing to trees and blobs should have already been processed."
                )
            }
            _ => err,
        },
    }
}

/// Convert commit IDs in error messages from Bonsai to Git
async fn update_error(mappings_store: &GitMappingsStore, err: MononokeError) -> anyhow::Error {
    match err {
        MononokeError::NonFastForwardMove { bookmark, from, to } => {
            let from = git_sha_str(mappings_store, &from).await;
            let to = git_sha_str(mappings_store, &to).await;
            anyhow::anyhow!(
                "Non fast-forward bookmark move of '{bookmark}' from {from} to {to}\
            \n\nFor more information about hooks and bypassing, refer {HOOK_WIKI_LINK}"
            )
        }
        MononokeError::HookFailure(hook_rejections) => {
            let mut hook_msgs = vec![];
            for hook_rejection in hook_rejections {
                let git_sha = git_sha_str(mappings_store, &hook_rejection.cs_id).await;
                hook_msgs.push(format!(
                    "  {} for {}: {}",
                    hook_rejection.hook_name, git_sha, hook_rejection.reason.long_description
                ));
            }
            anyhow::anyhow!(
                "hooks failed:\n{}\n\nFor more information about hooks and bypassing, refer {}",
                hook_msgs.join("\n"),
                HOOK_WIKI_LINK
            )
        }
        e => e.into(),
    }
}

async fn git_sha_str(mappings_store: &GitMappingsStore, bonsai: &ChangesetId) -> String {
    if let Ok(Some(oid)) = mappings_store.get_git_sha1(bonsai).await {
        oid.to_hex().to_string()
    } else {
        bonsai.to_hex().to_string()
    }
}
