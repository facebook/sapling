/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use bookmarks_movement::BookmarkKindRestrictions;
use bookmarks_movement::BookmarkMovementError;
use bookmarks_movement::BookmarkUpdatePolicy;
use bookmarks_movement::BookmarkUpdateTargets;
use bytes::Bytes;
use context::CoreContext;
use hooks::HookManager;
use mercurial_mutation::HgMutationStoreRef;
use metaconfig_types::Address;
use metaconfig_types::PushrebaseRemoteMode;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use pushrebase::PushrebaseError;
#[cfg(fbcode_build)]
use pushrebase_client::LandServicePushrebaseClient;
use pushrebase_client::LocalPushrebaseClient;
use pushrebase_client::PushrebaseClient;
use reachabilityindex::LeastCommonAncestorsHint;
use repo_authorization::AuthorizationContext;
use repo_identity::RepoIdentityRef;
use repo_update_logger::log_new_commits;
use repo_update_logger::CommitInfo;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::debug;
use stats::prelude::*;
use tunables::tunables;

use crate::hook_running::map_hook_rejections;
use crate::hook_running::HookRejectionRemapper;
use crate::rate_limits::enforce_commit_rate_limits;
use crate::response::UnbundleBookmarkOnlyPushRebaseResponse;
use crate::response::UnbundleInfinitePushResponse;
use crate::response::UnbundlePushRebaseResponse;
use crate::response::UnbundlePushResponse;
use crate::response::UnbundleResponse;
use crate::BundleResolverError;
use crate::BundleResolverResultExt;
use crate::CrossRepoPushSource;
use crate::InfiniteBookmarkPush;
use crate::NonFastForwardPolicy;
use crate::PlainBookmarkPush;
use crate::PostResolveAction;
use crate::PostResolveBookmarkOnlyPushRebase;
use crate::PostResolveInfinitePush;
use crate::PostResolvePush;
use crate::PostResolvePushRebase;
use crate::PushrebaseBookmarkSpec;

define_stats! {
    prefix = "mononoke.unbundle.processed";
    push: dynamic_timeseries("{}.push", (reponame: String); Rate, Sum),
    pushrebase: dynamic_timeseries("{}.pushrebase", (reponame: String); Rate, Sum),
    bookmark_only_pushrebase: dynamic_timeseries("{}.bookmark_only_pushrebase", (reponame: String); Rate, Sum),
    infinitepush: dynamic_timeseries("{}.infinitepush", (reponame: String); Rate, Sum),
}

pub trait Repo = bookmarks_movement::Repo + HgMutationStoreRef;

pub async fn run_post_resolve_action(
    ctx: &CoreContext,
    repo: &impl Repo,
    lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
    hook_manager: &HookManager,
    action: PostResolveAction,
    cross_repo_push_source: CrossRepoPushSource,
) -> Result<UnbundleResponse, BundleResolverError> {
    enforce_commit_rate_limits(ctx, &action).await?;

    // FIXME: it's used not only in pushrebase, so it worth moving
    // populate_git_mapping outside of PushrebaseParams.
    let unbundle_response = match action {
        PostResolveAction::Push(action) => run_push(
            ctx,
            repo,
            lca_hint,
            hook_manager,
            action,
            cross_repo_push_source,
        )
        .await
        .context("While doing a push")
        .map(UnbundleResponse::Push)?,
        PostResolveAction::InfinitePush(action) => run_infinitepush(
            ctx,
            repo,
            lca_hint,
            hook_manager,
            action,
            cross_repo_push_source,
        )
        .await
        .context("While doing an infinitepush")
        .map(UnbundleResponse::InfinitePush)?,
        PostResolveAction::PushRebase(action) => run_pushrebase(
            ctx,
            repo,
            lca_hint,
            hook_manager,
            action,
            cross_repo_push_source,
        )
        .await
        .map(UnbundleResponse::PushRebase)?,
        PostResolveAction::BookmarkOnlyPushRebase(action) => run_bookmark_only_pushrebase(
            ctx,
            repo,
            lca_hint,
            hook_manager,
            action,
            cross_repo_push_source,
        )
        .await
        .context("While doing a bookmark-only pushrebase")
        .map(UnbundleResponse::BookmarkOnlyPushRebase)?,
    };
    report_unbundle_type(repo, &unbundle_response);
    Ok(unbundle_response)
}

fn report_unbundle_type(repo: &impl RepoIdentityRef, unbundle_response: &UnbundleResponse) {
    let repo_name = repo.repo_identity().name().to_string();
    match unbundle_response {
        UnbundleResponse::Push(_) => STATS::push.add_value(1, (repo_name,)),
        UnbundleResponse::PushRebase(_) => STATS::pushrebase.add_value(1, (repo_name,)),
        UnbundleResponse::InfinitePush(_) => STATS::infinitepush.add_value(1, (repo_name,)),
        UnbundleResponse::BookmarkOnlyPushRebase(_) => {
            STATS::bookmark_only_pushrebase.add_value(1, (repo_name,))
        }
    }
}

async fn run_push(
    ctx: &CoreContext,
    repo: &impl Repo,
    lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
    hook_manager: &HookManager,
    action: PostResolvePush,
    cross_repo_push_source: CrossRepoPushSource,
) -> Result<UnbundlePushResponse, BundleResolverError> {
    debug!(ctx.logger(), "unbundle processing: running push.");
    let PostResolvePush {
        changegroup_id,
        mut bookmark_pushes,
        mutations,
        maybe_pushvars,
        non_fast_forward_policy,
        uploaded_bonsais,
        uploaded_hg_changeset_ids,
        hook_rejection_remapper,
    } = action;

    if tunables().get_mutation_accept_for_infinitepush() {
        repo.hg_mutation_store()
            .add_entries(ctx, uploaded_hg_changeset_ids, mutations)
            .await
            .context("Failed to store mutation data")?;
    }

    if bookmark_pushes.len() > 1 {
        return Err(anyhow!(
            "only push to at most one bookmark is allowed, got {:?}",
            bookmark_pushes
        )
        .into());
    }

    let mut changesets_to_log = vec![];
    let mut new_changesets = HashMap::new();
    for bcs in uploaded_bonsais {
        let changeset_id = bcs.get_changeset_id();
        changesets_to_log.push(CommitInfo::new(&bcs, None));
        new_changesets.insert(changeset_id, bcs);
    }

    let mut bookmark_ids = Vec::new();
    let mut maybe_bookmark = None;
    if let Some(bookmark_push) = bookmark_pushes.pop() {
        bookmark_ids.push(bookmark_push.part_id);

        plain_push_bookmark(
            ctx,
            repo,
            lca_hint,
            hook_manager,
            &bookmark_push,
            new_changesets,
            non_fast_forward_policy,
            BookmarkUpdateReason::Push,
            maybe_pushvars.as_ref(),
            hook_rejection_remapper.as_ref(),
            cross_repo_push_source,
        )
        .await?;

        maybe_bookmark = Some(bookmark_push.name);
    }

    // Since this is a normal push, any bookmark must be public.
    log_new_commits(
        ctx,
        repo,
        maybe_bookmark
            .as_ref()
            .map(|name| (name, BookmarkKind::Publishing)),
        changesets_to_log,
    )
    .await;

    Ok(UnbundlePushResponse {
        changegroup_id,
        bookmark_ids,
    })
}

async fn run_infinitepush(
    ctx: &CoreContext,
    repo: &impl Repo,
    lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
    hook_manager: &HookManager,
    action: PostResolveInfinitePush,
    cross_repo_push_source: CrossRepoPushSource,
) -> Result<UnbundleInfinitePushResponse, BundleResolverError> {
    debug!(ctx.logger(), "unbundle processing: running infinitepush");
    let PostResolveInfinitePush {
        changegroup_id,
        maybe_bookmark_push,
        mutations,
        uploaded_bonsais,
        uploaded_hg_changeset_ids,
    } = action;

    if tunables().get_mutation_accept_for_infinitepush() {
        repo.hg_mutation_store()
            .add_entries(ctx, uploaded_hg_changeset_ids, mutations)
            .await
            .context("Failed to store mutation data")?;
    }

    let bookmark = match maybe_bookmark_push {
        Some(bookmark_push) => {
            infinitepush_scratch_bookmark(
                ctx,
                repo,
                lca_hint,
                hook_manager,
                &bookmark_push,
                cross_repo_push_source,
            )
            .await?;

            Some(bookmark_push.name)
        }
        None => None,
    };

    let changesets_to_log = uploaded_bonsais
        .iter()
        .map(|bcs| CommitInfo::new(bcs, None))
        .collect();
    // Since this is an infinitepush, any bookmark must be a scratch bookmark.
    log_new_commits(
        ctx,
        repo,
        bookmark.as_ref().map(|name| (name, BookmarkKind::Scratch)),
        changesets_to_log,
    )
    .await;

    Ok(UnbundleInfinitePushResponse { changegroup_id })
}

async fn run_pushrebase(
    ctx: &CoreContext,
    repo: &impl Repo,
    lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
    hook_manager: &HookManager,
    action: PostResolvePushRebase,
    cross_repo_push_source: CrossRepoPushSource,
) -> Result<UnbundlePushRebaseResponse, BundleResolverError> {
    debug!(ctx.logger(), "unbundle processing: running pushrebase.");
    let PostResolvePushRebase {
        bookmark_push_part_id,
        bookmark_spec,
        maybe_pushvars,
        commonheads,
        uploaded_bonsais,
        hook_rejection_remapper,
    } = action;

    let (bookmark, pushrebased_rev, pushrebased_changesets) = match bookmark_spec {
        // There's no `.context()` after `normal_pushrebase`, as it has
        // `Error=BundleResolverError` and doing `.context("bla").from_err()`
        // would turn some useful variant of `BundleResolverError` into generic
        // `BundleResolverError::Error`, which in turn would render incorrectly
        // (see definition of `BundleResolverError`).
        PushrebaseBookmarkSpec::NormalPushrebase(onto_bookmark) => {
            let mut changesets_to_log: HashMap<_, _> = uploaded_bonsais
                .iter()
                .map(|bcs| (bcs.get_changeset_id(), CommitInfo::new(bcs, None)))
                .collect();

            let (pushrebased_rev, pushrebased_changesets) = normal_pushrebase(
                ctx,
                repo,
                lca_hint,
                uploaded_bonsais,
                &onto_bookmark,
                maybe_pushvars.as_ref(),
                hook_manager,
                hook_rejection_remapper.as_ref(),
                cross_repo_push_source,
            )
            .await?;
            // Modify the changeset logs with the newly pushrebased hashes.
            for pair in pushrebased_changesets.iter() {
                let info = changesets_to_log
                    .get_mut(&pair.id_old)
                    .ok_or_else(|| anyhow!("Missing commit info for {}", pair.id_old))?;
                info.update_changeset_id(pair.id_old, pair.id_new)?;
            }
            // Wireprotocol pushrebase is always for public bookmarks
            log_new_commits(
                ctx,
                repo,
                Some((&onto_bookmark, BookmarkKind::Publishing)),
                changesets_to_log.into_values().collect(),
            )
            .await;
            (onto_bookmark, pushrebased_rev, pushrebased_changesets)
        }
        PushrebaseBookmarkSpec::ForcePushrebase(plain_push) => {
            let changesets_to_log = uploaded_bonsais
                .iter()
                .map(|bcs| CommitInfo::new(bcs, None))
                .collect();

            let pushrebased_rev = force_pushrebase(
                ctx,
                repo,
                lca_hint,
                hook_manager,
                uploaded_bonsais,
                &plain_push,
                maybe_pushvars.as_ref(),
                hook_rejection_remapper.as_ref(),
                cross_repo_push_source,
            )
            .await
            .context("While doing a force pushrebase")?;
            // Wireprotocol pushrebase is always for public bookmarks
            log_new_commits(
                ctx,
                repo,
                Some((&plain_push.name, BookmarkKind::Publishing)),
                changesets_to_log,
            )
            .await;
            // Force pushrebase merely force-moves the bookmark, it does not rebase any commits.
            (plain_push.name, pushrebased_rev, Vec::new())
        }
    };

    repo.phases()
        .add_reachable_as_public(ctx, vec![pushrebased_rev.clone()])
        .await
        .context("While marking pushrebased changeset as public")?;

    Ok(UnbundlePushRebaseResponse {
        commonheads,
        pushrebased_rev,
        pushrebased_changesets,
        onto: bookmark,
        bookmark_push_part_id,
    })
}

async fn run_bookmark_only_pushrebase(
    ctx: &CoreContext,
    repo: &impl Repo,
    lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
    hook_manager: &HookManager,
    action: PostResolveBookmarkOnlyPushRebase,
    cross_repo_push_source: CrossRepoPushSource,
) -> Result<UnbundleBookmarkOnlyPushRebaseResponse, BundleResolverError> {
    debug!(
        ctx.logger(),
        "unbundle processing: running bookmark-only pushrebase."
    );
    let PostResolveBookmarkOnlyPushRebase {
        bookmark_push,
        maybe_pushvars,
        non_fast_forward_policy,
        hook_rejection_remapper,
    } = action;

    let part_id = bookmark_push.part_id;

    // This is a bookmark-only push, so there are no new changesets.
    let new_changesets = HashMap::new();

    plain_push_bookmark(
        ctx,
        repo,
        lca_hint,
        hook_manager,
        &bookmark_push,
        new_changesets,
        non_fast_forward_policy,
        BookmarkUpdateReason::Pushrebase,
        maybe_pushvars.as_ref(),
        hook_rejection_remapper.as_ref(),
        cross_repo_push_source,
    )
    .await?;

    Ok(UnbundleBookmarkOnlyPushRebaseResponse {
        bookmark_push_part_id: part_id,
    })
}

async fn convert_bookmark_movement_err(
    err: BookmarkMovementError,
    hook_rejection_remapper: &dyn HookRejectionRemapper,
) -> Result<BundleResolverError> {
    Ok(match err {
        BookmarkMovementError::PushrebaseError(PushrebaseError::Conflicts(conflicts)) => {
            BundleResolverError::PushrebaseConflicts(conflicts)
        }
        BookmarkMovementError::HookFailure(rejections) => {
            let rejections = map_hook_rejections(rejections, hook_rejection_remapper).await?;
            BundleResolverError::HookError(rejections)
        }
        _ => BundleResolverError::Error(err.into()),
    })
}

pub fn maybe_client_from_address<'a>(
    remote_mode: &'a PushrebaseRemoteMode,
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
) -> Option<Box<dyn PushrebaseClient + 'a>> {
    match remote_mode {
        PushrebaseRemoteMode::RemoteLandService(address)
        | PushrebaseRemoteMode::RemoteLandServiceWithLocalFallback(address) => {
            address_from_land_service(address, ctx, repo)
        }
        PushrebaseRemoteMode::Local => None,
    }
}

fn address_from_land_service<'a>(
    address: &'a Address,
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
) -> Option<Box<dyn PushrebaseClient + 'a>> {
    #[cfg(fbcode_build)]
    {
        match address {
            metaconfig_types::Address::Tier(tier) => Some(Box::new(
                LandServicePushrebaseClient::from_tier(ctx, tier.clone(), repo).ok()?,
            )),
            metaconfig_types::Address::HostPort(host_port) => Some(Box::new(
                LandServicePushrebaseClient::from_host_port(ctx, host_port.clone(), repo).ok()?,
            )),
        }
    }
    #[cfg(not(fbcode_build))]
    {
        let _ = (address, ctx, repo);
        unreachable!()
    }
}

async fn normal_pushrebase<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
    changesets: HashSet<BonsaiChangeset>,
    bookmark: &'a BookmarkName,
    maybe_pushvars: Option<&'a HashMap<String, Bytes>>,
    hook_manager: &'a HookManager,
    hook_rejection_remapper: &'a dyn HookRejectionRemapper,
    cross_repo_push_source: CrossRepoPushSource,
) -> Result<(ChangesetId, Vec<pushrebase::PushrebaseChangesetPair>), BundleResolverError> {
    let bookmark_restriction = BookmarkKindRestrictions::OnlyPublishing;
    let remote_mode = if tunables().get_force_local_pushrebase() {
        PushrebaseRemoteMode::Local
    } else {
        repo.repo_config().pushrebase.remote_mode.clone()
    };
    let maybe_fallback_scuba: Option<(MononokeScubaSampleBuilder, BookmarkMovementError)> = {
        let maybe_client: Option<Box<dyn PushrebaseClient>> =
            maybe_client_from_address(&remote_mode, ctx, repo);

        if let Some(client) = maybe_client {
            let result = client
                .pushrebase(
                    bookmark,
                    changesets.clone(),
                    maybe_pushvars,
                    cross_repo_push_source,
                    bookmark_restriction,
                )
                .await;
            match (result, &remote_mode) {
                (Ok(outcome), _) => {
                    return Ok((outcome.head, outcome.rebased_changesets));
                }
                // No fallback, propagate error
                (Err(err), metaconfig_types::PushrebaseRemoteMode::RemoteLandService(..)) => {
                    return Err(convert_bookmark_movement_err(err, hook_rejection_remapper).await?);
                }
                (Err(err), _) => {
                    slog::warn!(
                        ctx.logger(),
                        "Failed to pushrebase remotely, falling back to local. Error: {}",
                        err
                    );
                    let mut scuba = ctx.scuba().clone();
                    scuba.add("bookmark_name", bookmark.as_str());
                    scuba.add(
                        "changeset_id",
                        changesets
                            .iter()
                            .next()
                            .map(|b| b.get_changeset_id().to_string()),
                    );
                    Some((scuba, err))
                }
            }
        } else {
            None
        }
    };
    let authz = AuthorizationContext::new(ctx);
    let result = LocalPushrebaseClient {
        ctx,
        authz: &authz,
        repo,
        lca_hint,
        hook_manager,
    }
    .pushrebase(
        bookmark,
        changesets,
        maybe_pushvars,
        cross_repo_push_source,
        bookmark_restriction,
    )
    .await;
    if let Some((mut scuba, err)) = maybe_fallback_scuba {
        if result.is_ok() {
            scuba.log_with_msg("failed_remote_pushrebase", err.to_string());
        }
    }

    match result {
        Ok(outcome) => Ok((outcome.head, outcome.rebased_changesets)),
        Err(err) => Err(convert_bookmark_movement_err(err, hook_rejection_remapper).await?),
    }
}

async fn force_pushrebase(
    ctx: &CoreContext,
    repo: &impl Repo,
    lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
    hook_manager: &HookManager,
    uploaded_bonsais: HashSet<BonsaiChangeset>,
    bookmark_push: &PlainBookmarkPush<ChangesetId>,
    maybe_pushvars: Option<&HashMap<String, Bytes>>,
    hook_rejection_remapper: &dyn HookRejectionRemapper,
    cross_repo_push_source: CrossRepoPushSource,
) -> Result<ChangesetId, BundleResolverError> {
    let new_target = bookmark_push
        .new
        .ok_or_else(|| anyhow!("new changeset is required for force pushrebase"))?;

    let mut new_changesets = HashMap::new();
    for bcs in uploaded_bonsais {
        let cs_id = bcs.get_changeset_id();
        new_changesets.insert(cs_id, bcs);
    }

    plain_push_bookmark(
        ctx,
        repo,
        lca_hint,
        hook_manager,
        bookmark_push,
        new_changesets,
        NonFastForwardPolicy::Allowed,
        BookmarkUpdateReason::Pushrebase,
        maybe_pushvars,
        hook_rejection_remapper,
        cross_repo_push_source,
    )
    .await?;

    Ok(new_target)
}

async fn plain_push_bookmark(
    ctx: &CoreContext,
    repo: &impl Repo,
    lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
    hook_manager: &HookManager,
    bookmark_push: &PlainBookmarkPush<ChangesetId>,
    new_changesets: HashMap<ChangesetId, BonsaiChangeset>,
    non_fast_forward_policy: NonFastForwardPolicy,
    reason: BookmarkUpdateReason,
    maybe_pushvars: Option<&HashMap<String, Bytes>>,
    hook_rejection_remapper: &dyn HookRejectionRemapper,
    cross_repo_push_source: CrossRepoPushSource,
) -> Result<(), BundleResolverError> {
    match (bookmark_push.old, bookmark_push.new) {
        (None, Some(new_target)) => {
            let res =
                bookmarks_movement::CreateBookmarkOp::new(&bookmark_push.name, new_target, reason)
                    .only_if_public()
                    .with_new_changesets(new_changesets)
                    .with_pushvars(maybe_pushvars)
                    .with_push_source(cross_repo_push_source)
                    .only_log_acl_checks(tunables().get_log_only_wireproto_write_acl())
                    .run(
                        ctx,
                        &AuthorizationContext::new(ctx),
                        repo,
                        lca_hint,
                        hook_manager,
                    )
                    .await;
            match res {
                Ok(()) => {}
                Err(err) => match err {
                    BookmarkMovementError::HookFailure(rejections) => {
                        let rejections =
                            map_hook_rejections(rejections, hook_rejection_remapper).await?;
                        return Err(BundleResolverError::HookError(rejections));
                    }
                    _ => {
                        return Err(BundleResolverError::Error(
                            Error::from(err).context("Failed to create bookmark"),
                        ));
                    }
                },
            }
        }

        (Some(old_target), Some(new_target)) => {
            let res = bookmarks_movement::UpdateBookmarkOp::new(
                &bookmark_push.name,
                BookmarkUpdateTargets {
                    old: old_target,
                    new: new_target,
                },
                if non_fast_forward_policy == NonFastForwardPolicy::Allowed {
                    BookmarkUpdatePolicy::AnyPermittedByConfig
                } else {
                    BookmarkUpdatePolicy::FastForwardOnly
                },
                reason,
            )
            .only_if_public()
            .with_new_changesets(new_changesets)
            .with_pushvars(maybe_pushvars)
            .with_push_source(cross_repo_push_source)
            .only_log_acl_checks(tunables().get_log_only_wireproto_write_acl())
            .run(
                ctx,
                &AuthorizationContext::new(ctx),
                repo,
                lca_hint,
                hook_manager,
            )
            .await;
            match res {
                Ok(()) => {}
                Err(err) => match err {
                    BookmarkMovementError::HookFailure(rejections) => {
                        let rejections =
                            map_hook_rejections(rejections, hook_rejection_remapper).await?;
                        return Err(BundleResolverError::HookError(rejections));
                    }
                    _ => {
                        return Err(BundleResolverError::Error(Error::from(err).context(
                            if non_fast_forward_policy == NonFastForwardPolicy::Allowed {
                                "Failed to move bookmark"
                            } else {
                                concat!(
                                    "Failed to fast-forward bookmark (set pushvar ",
                                    "NON_FAST_FORWARD=true for a non-fast-forward move)",
                                )
                            },
                        )));
                    }
                },
            }
        }

        (Some(old_target), None) => {
            bookmarks_movement::DeleteBookmarkOp::new(&bookmark_push.name, old_target, reason)
                .only_if_public()
                .with_pushvars(maybe_pushvars)
                .only_log_acl_checks(tunables().get_log_only_wireproto_write_acl())
                .run(ctx, &AuthorizationContext::new(ctx), repo)
                .await
                .context("Failed to delete bookmark")?;
        }

        (None, None) => {}
    }
    Ok(())
}

async fn infinitepush_scratch_bookmark(
    ctx: &CoreContext,
    repo: &impl Repo,
    lca_hint: &Arc<dyn LeastCommonAncestorsHint>,
    hook_manager: &HookManager,
    bookmark_push: &InfiniteBookmarkPush<ChangesetId>,
    cross_repo_push_source: CrossRepoPushSource,
) -> Result<()> {
    if bookmark_push.old.is_none() && bookmark_push.create {
        bookmarks_movement::CreateBookmarkOp::new(
            &bookmark_push.name,
            bookmark_push.new,
            BookmarkUpdateReason::Push,
        )
        .only_if_scratch()
        .with_push_source(cross_repo_push_source)
        .only_log_acl_checks(tunables().get_log_only_wireproto_write_acl())
        .run(
            ctx,
            &AuthorizationContext::new(ctx),
            repo,
            lca_hint,
            hook_manager,
        )
        .await
        .context("Failed to create scratch bookmark")?;
    } else {
        let old_target = bookmark_push.old.ok_or_else(|| {
            anyhow!(
                "Unknown bookmark: {}. Use --create to create one.",
                bookmark_push.name
            )
        })?;
        bookmarks_movement::UpdateBookmarkOp::new(
            &bookmark_push.name,
            BookmarkUpdateTargets {
                old: old_target,
                new: bookmark_push.new,
            },
            if bookmark_push.force {
                BookmarkUpdatePolicy::AnyPermittedByConfig
            } else {
                BookmarkUpdatePolicy::FastForwardOnly
            },
            BookmarkUpdateReason::Push,
        )
        .only_if_scratch()
        .with_push_source(cross_repo_push_source)
        .only_log_acl_checks(tunables().get_log_only_wireproto_write_acl())
        .run(
            ctx,
            &AuthorizationContext::new(ctx),
            repo,
            lca_hint,
            hook_manager,
        )
        .await
        .context(if bookmark_push.force {
            "Failed to move scratch bookmark"
        } else {
            "Failed to fast-forward scratch bookmark (try --force?)"
        })?;
    }

    Ok(())
}
