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
use cross_repo_sync::get_all_submodule_deps_from_repo_pair;
use cross_repo_sync::CommitSyncRepos;
use cross_repo_sync::CommitSyncer;
use cross_repo_sync::RepoProvider;
use cross_repo_sync::Syncers;
use mononoke_app::args::RepoArg;
use mononoke_app::MononokeApp;
use sql_query_config::SqlQueryConfigArc;

pub trait CrossRepo = cross_repo_sync::Repo
    + SqlQueryConfigArc
    + for<'a> facet::AsyncBuildable<'a, repo_factory::RepoFactoryBuilder<'a>>;

pub async fn create_commit_syncers_from_app<R: CrossRepo>(
    ctx: &CoreContext,
    app: &MononokeApp,
    source_repo: R,
    target_repo: R,
) -> Result<Syncers<R>, Error> {
    let repo_provider = repo_provider_from_mononoke_app(app);

    let submodule_deps = get_all_submodule_deps_from_repo_pair(
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

pub async fn create_single_direction_commit_syncer<R: CrossRepo>(
    ctx: &CoreContext,
    app: &MononokeApp,
    source_repo: R,
    target_repo: R,
) -> Result<CommitSyncer<R>, Error> {
    let repo_provider = repo_provider_from_mononoke_app(app);

    let submodule_deps = get_all_submodule_deps_from_repo_pair(
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
    let commit_sync_repos = CommitSyncRepos::new(source_repo, target_repo, submodule_deps)?;

    Ok(CommitSyncer::new(
        ctx,
        commit_sync_repos,
        live_commit_sync_config,
    ))
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
