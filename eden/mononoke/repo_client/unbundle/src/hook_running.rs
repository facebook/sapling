/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use blobrepo::BlobRepo;
use bookmarks_movement::BookmarkMovementError;
use context::CoreContext;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use hooks::CrossRepoPushSource;
use hooks::HookManager;
use hooks::HookRejection;
use hooks::PushAuthoredBy;
use mercurial_derived_data::DeriveHgChangeset;

use crate::resolver::HgHookRejection;
use crate::resolver::PostResolveAction;
use crate::resolver::PostResolvePushRebase;
use crate::BundleResolverError;

/// A function to remap hook rejections from Bonsai to Hg.
pub(crate) trait HookRejectionRemapper = (Fn(HookRejection) -> BoxFuture<'static, Result<HgHookRejection, Error>>)
    + Send
    + Sync
    + 'static;

pub(crate) fn make_hook_rejection_remapper(
    ctx: &CoreContext,
    repo: BlobRepo,
) -> Box<dyn HookRejectionRemapper> {
    let ctx = ctx.clone();
    Box::new(
        move |HookRejection {
                  hook_name,
                  cs_id,
                  reason,
              }| {
            let ctx = ctx.clone();
            let repo = repo.clone();
            async move {
                let hg_cs_id = repo.derive_hg_changeset(&ctx, cs_id).await?;
                Ok(HgHookRejection {
                    hook_name,
                    hg_cs_id,
                    reason,
                })
            }
            .boxed()
        },
    )
}

pub(crate) async fn map_hook_rejections(
    rejections: Vec<HookRejection>,
    hook_rejection_remapper: &dyn HookRejectionRemapper,
) -> Result<Vec<HgHookRejection>, Error> {
    stream::iter(rejections)
        .map(move |rejection| async move { (*hook_rejection_remapper)(rejection).await })
        .buffered(10)
        .try_collect()
        .await
        .context("Failed to remap hook rejections")
}

pub async fn run_hooks(
    ctx: &CoreContext,
    repo: &BlobRepo,
    hook_manager: &HookManager,
    action: &PostResolveAction,
    cross_repo_push_source: CrossRepoPushSource,
) -> Result<(), BundleResolverError> {
    match action {
        // TODO: Need to run hooks on Push, not just Pushrebase
        PostResolveAction::Push(_) => Ok(()),
        PostResolveAction::InfinitePush(_) => Ok(()),
        PostResolveAction::BookmarkOnlyPushRebase(_) => Ok(()),
        PostResolveAction::PushRebase(action) => {
            run_pushrebase_hooks(ctx, repo, hook_manager, action, cross_repo_push_source).await
        }
    }
}

async fn run_pushrebase_hooks(
    ctx: &CoreContext,
    repo: &BlobRepo,
    hook_manager: &HookManager,
    action: &PostResolvePushRebase,
    cross_repo_push_source: CrossRepoPushSource,
) -> Result<(), BundleResolverError> {
    match bookmarks_movement::run_hooks(
        ctx,
        hook_manager,
        action.bookmark_spec.get_bookmark_name(),
        action.uploaded_bonsais.iter(),
        action.maybe_pushvars.as_ref(),
        cross_repo_push_source,
        PushAuthoredBy::User,
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(BookmarkMovementError::HookFailure(rejections)) => {
            let hook_rejection_remapper = make_hook_rejection_remapper(ctx, repo.clone());
            let rejections =
                map_hook_rejections(rejections, hook_rejection_remapper.as_ref()).await?;
            Err(BundleResolverError::HookError(rejections))
        }
        Err(e) => Err(Error::from(e).into()),
    }
}
