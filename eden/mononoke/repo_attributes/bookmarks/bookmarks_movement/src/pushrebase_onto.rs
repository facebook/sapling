/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::anyhow;
use bookmarks::BookmarkUpdateReason;
use bookmarks_types::BookmarkKey;
use bookmarks_types::BookmarkKind;
use bytes::Bytes;
use context::CoreContext;
use hooks::CrossRepoPushSource;
use hooks::HookManager;
use metaconfig_types::LAND_INSTANCE_ID_PUSHVAR_KEY;
use metaconfig_types::MergeResolutionOverride;
use metaconfig_types::PHAB_DIFF_ID_PUSHVAR_KEY;
use metaconfig_types::PushrebaseFlags;
use metaconfig_types::RepoConfigRef;
use mononoke_types::BonsaiChangeset;
use pushrebase_hook::PushrebaseHook;
use pushrebase_hooks::get_pushrebase_hooks;
use repo_authorization::AuthorizationContext;
use repo_authorization::RepoWriteOperation;
use repo_bookmark_attrs::RepoBookmarkAttrsRef;
use repo_update_logger::BookmarkInfo;
use repo_update_logger::BookmarkOperation;
use repo_update_logger::CommitInfo;
use repo_update_logger::log_bookmark_operation;
use repo_update_logger::log_new_commits;

use crate::BookmarkMovementError;
use crate::Repo;
use crate::affected_changesets::AdditionalChangesets;
use crate::affected_changesets::AffectedChangesets;
use crate::repo_lock::RepoLockPushrebaseHook;
use crate::repo_lock::check_repo_lock;
use crate::restrictions::BookmarkKindRestrictions;
use crate::restrictions::check_bookmark_sync_config;

/// Returns the configured pushrebase flags with bookmark and request overrides.
pub fn pushrebase_flags(
    repo: &(impl RepoConfigRef + RepoBookmarkAttrsRef),
    bookmark: &BookmarkKey,
    pushvars: Option<&HashMap<String, Bytes>>,
) -> PushrebaseFlags {
    let mut flags = repo.repo_config().pushrebase.flags.clone();
    if let Some(rewritedates) = repo.repo_bookmark_attrs().should_rewrite_dates(bookmark) {
        flags.rewritedates = rewritedates;
    }

    flags.merge_resolution_override = MergeResolutionOverride::from_pushvar_value(
        pushvars
            .and_then(|p| p.get(MergeResolutionOverride::PUSHVAR_KEY))
            .map(|b| b.as_ref()),
    );
    flags.land_instance_id = pushvars
        .and_then(|p| p.get(LAND_INSTANCE_ID_PUSHVAR_KEY))
        .and_then(|b| std::str::from_utf8(b.as_ref()).ok())
        .map(str::to_owned);
    flags.phab_diff_id = pushvars
        .and_then(|p| p.get(PHAB_DIFF_ID_PUSHVAR_KEY))
        .and_then(|b| std::str::from_utf8(b.as_ref()).ok())
        .map(str::to_owned);
    flags
}

/// Authorizes and validates the changesets supplied to pushrebase.
pub async fn validate_pushrebase_request(
    ctx: &CoreContext,
    authz: &AuthorizationContext,
    repo: &impl Repo,
    hook_manager: &HookManager,
    bookmark: &BookmarkKey,
    changesets: &[BonsaiChangeset],
    pushvars: Option<&HashMap<String, Bytes>>,
    cross_repo_push_source: CrossRepoPushSource,
    bookmark_restrictions: BookmarkKindRestrictions,
) -> Result<BookmarkKind, BookmarkMovementError> {
    let kind = bookmark_restrictions.check_kind(repo, bookmark)?;

    authz
        .require_repo_write(ctx, repo, RepoWriteOperation::LandStack(kind))
        .await?;
    authz.require_bookmark_modify(ctx, repo, bookmark).await?;

    check_bookmark_sync_config(ctx, repo, bookmark, kind).await?;

    if repo.repo_config().pushrebase.block_merges {
        let any_merges = changesets.iter().any(BonsaiChangeset::is_merge);
        if any_merges {
            return Err(anyhow!(
                "Pushrebase blocked because it contains a merge commit.\n\
                If you need this for a specific use case please contact\n\
                the Source Control team at https://fburl.com/27qnuyl2"
            )
            .into());
        }
    }

    let reason = BookmarkUpdateReason::Pushrebase;

    AffectedChangesets::with_source_changesets(changesets)
        .check_restrictions(
            ctx,
            authz,
            repo,
            hook_manager,
            bookmark,
            pushvars,
            reason,
            kind,
            AdditionalChangesets::None,
            cross_repo_push_source,
            None,
        )
        .await?;

    Ok(kind)
}

/// Builds the runtime hooks after checking the repository lock.
pub async fn prepare_pushrebase_hooks(
    ctx: &CoreContext,
    authz: &AuthorizationContext,
    repo: &impl Repo,
    bookmark: &BookmarkKey,
    pushvars: Option<&HashMap<String, Bytes>>,
    kind: BookmarkKind,
) -> Result<Vec<Box<dyn PushrebaseHook>>, BookmarkMovementError> {
    let mut pushrebase_hooks =
        get_pushrebase_hooks(ctx, repo, bookmark, &repo.repo_config().pushrebase, None).await?;

    // For pushrebase, we check the repo lock once at the beginning of the
    // pushrebase operation, and then once more as part of the pushrebase
    // bookmark update transaction, to check if the repo got locked while
    // we were performing the pushrebase.
    check_repo_lock(
        ctx,
        repo,
        kind,
        pushvars,
        ctx.metadata().identities(),
        authz,
    )
    .await?;

    if let Some(hook) = RepoLockPushrebaseHook::new(
        repo.repo_identity().id(),
        kind,
        pushvars,
        repo.repo_permission_checker(),
        ctx.metadata().identities(),
        authz,
    )
    .await
    {
        pushrebase_hooks.push(hook);
    }

    Ok(pushrebase_hooks)
}

/// Performs all post-pushrebase work: scribe logging, bookmark operation
/// logging, and phase marking.
pub async fn postprocess_pushrebase_outcome(
    ctx: &CoreContext,
    repo: &impl Repo,
    bookmark: &BookmarkKey,
    kind: BookmarkKind,
    outcome: &pushrebase::PushrebaseOutcome,
    source_changesets: &HashSet<BonsaiChangeset>,
    log_new_public_commits_to_scribe: bool,
) -> Result<(), BookmarkMovementError> {
    if log_new_public_commits_to_scribe {
        let mut changesets_to_log: HashMap<_, _> = source_changesets
            .iter()
            .map(|bcs| (bcs.get_changeset_id(), CommitInfo::new(bcs, None)))
            .collect();

        for pair in outcome.rebased_changesets.iter() {
            let info = changesets_to_log
                .get_mut(&pair.id_old)
                .ok_or_else(|| anyhow!("Missing commit info for {}", pair.id_old))?;
            info.update_changeset_id(pair.id_old, pair.id_new)?;
        }

        log_new_commits(
            ctx,
            repo,
            Some((bookmark, kind)),
            changesets_to_log.into_values().collect(),
        )
        .await;
    }

    let reason = BookmarkUpdateReason::Pushrebase;
    let info = BookmarkInfo {
        bookmark_name: bookmark.clone(),
        bookmark_kind: kind,
        operation: BookmarkOperation::Pushrebase(outcome.old_bookmark_value, outcome.head),
        reason,
    };
    log_bookmark_operation(ctx, repo, &info).await;

    // Marking the pushrebased changeset as public.
    if kind.is_public() {
        repo.phases()
            .add_reachable_as_public(ctx, vec![outcome.head.clone()])
            .await?;
    }

    Ok(())
}
