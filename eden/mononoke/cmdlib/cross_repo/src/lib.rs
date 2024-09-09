/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Scaffolding that's generally useful to build CLI tools on top of Mononoke.

#![feature(trait_alias)]
use std::sync::Arc;

use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use context::CoreContext;
use cross_repo_sync::create_commit_syncers;
use cross_repo_sync::get_all_submodule_deps;
use cross_repo_sync::RepoProvider;
use cross_repo_sync::Syncers;
use futures::future;
use mononoke_app::args::RepoArg;
use mononoke_app::args::SourceAndTargetRepoArgs;
use mononoke_app::MononokeApp;
use sql_query_config::SqlQueryConfigArc;

pub trait CrossRepo = cross_repo_sync::Repo
    + SqlQueryConfigArc
    + for<'a> facet::AsyncBuildable<'a, repo_factory::RepoFactoryBuilder<'a>>;

/// Instantiate the `Syncers` struct by parsing `app`
pub async fn create_commit_syncers_from_app<R: CrossRepo>(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_args: &SourceAndTargetRepoArgs,
) -> Result<Syncers<R>, Error> {
    create_commit_syncers_from_app_impl(ctx, app, repo_args, false).await
}

pub async fn create_commit_syncers_from_app_unredacted<R: CrossRepo>(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_args: &SourceAndTargetRepoArgs,
) -> Result<Syncers<R>, Error> {
    create_commit_syncers_from_app_impl(ctx, app, repo_args, true).await
}

async fn create_commit_syncers_from_app_impl<R: CrossRepo>(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_args: &SourceAndTargetRepoArgs,
    unredacted: bool,
) -> Result<Syncers<R>, Error> {
    let (source_repo, target_repo): (R, R) = if !unredacted {
        app.open_source_and_target_repos(repo_args).await?
    } else {
        future::try_join(
            app.open_repo_unredacted(&repo_args.source_repo),
            app.open_repo_unredacted(&repo_args.target_repo),
        )
        .await?
    };

    let repo_provider = repo_provider_from_mononoke_app(app);

    let submodule_deps = get_all_submodule_deps(
        ctx,
        Arc::new(source_repo.clone()),
        Arc::new(target_repo.clone()),
        repo_provider,
    )
    .await?;

    let live_commit_sync_config = source_repo
        .repo_cross_repo()
        .live_commit_sync_config()
        .clone();

    let common_config =
        live_commit_sync_config.get_common_config(source_repo.repo_identity().id())?;

    let large_repo_id = common_config.large_repo_id;
    let source_repo_id = source_repo.repo_identity().id();
    let target_repo_id = target_repo.repo_identity().id();

    let (small_repo, large_repo) = if large_repo_id == source_repo_id {
        (target_repo, source_repo)
    } else if large_repo_id == target_repo_id {
        (source_repo, target_repo)
    } else {
        bail!(
            "Unexpectedly CommitSyncConfig {:?} has neither of {}, {} as a large repo",
            common_config,
            source_repo_id,
            target_repo_id
        );
    };

    create_commit_syncers(
        ctx,
        small_repo,
        large_repo,
        submodule_deps,
        live_commit_sync_config,
    )
}

pub fn repo_provider_from_mononoke_app<'a, R: CrossRepo>(
    app: &'a MononokeApp,
) -> RepoProvider<'a, R> {
    let repo_provider: RepoProvider<'a, R> = Arc::new(move |repo_id| {
        Box::pin(async move {
            let repo = app.open_repo_unredacted::<R>(&RepoArg::Id(repo_id)).await?;
            Ok(Arc::new(repo))
        })
    });

    repo_provider
}
