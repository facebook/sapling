/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Scaffolding that's generally useful to build CLI tools on top of Mononoke.

#![feature(trait_alias)]
use std::collections::HashMap;
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Error;
use anyhow::Result;
use cacheblob::LeaseOps;
use cloned::cloned;
use context::CoreContext;
use cross_repo_sync::create_commit_syncer_lease;
use cross_repo_sync::create_commit_syncers;
use cross_repo_sync::CommitSyncRepos;
use cross_repo_sync::CommitSyncer;
use cross_repo_sync::Source;
use cross_repo_sync::SubmoduleDeps;
use cross_repo_sync::Syncers;
use cross_repo_sync::Target;
use futures::future;
use futures::stream;
use futures::Future;
use futures::StreamExt;
use futures::TryStreamExt;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use live_commit_sync_config::LiveCommitSyncConfig;
use mononoke_app::args::AsRepoArg;
use mononoke_app::args::RepoArg;
use mononoke_app::args::SourceAndTargetRepoArgs;
use mononoke_app::MononokeApp;
use mononoke_types::NonRootMPath;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use synced_commit_mapping::SqlSyncedCommitMapping;

pub trait CrossRepo =
    cross_repo_sync::Repo + for<'a> facet::AsyncBuildable<'a, repo_factory::RepoFactoryBuilder<'a>>;

/// Instantiate the `Syncers` struct by parsing `app`
pub async fn create_commit_syncers_from_app<R: CrossRepo>(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_args: &SourceAndTargetRepoArgs,
) -> Result<Syncers<SqlSyncedCommitMapping, R>, Error> {
    create_commit_syncers_from_app_impl(ctx, app, repo_args, false).await
}

pub async fn create_commit_syncers_from_app_unredacted<R: CrossRepo>(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_args: &SourceAndTargetRepoArgs,
) -> Result<Syncers<SqlSyncedCommitMapping, R>, Error> {
    create_commit_syncers_from_app_impl(ctx, app, repo_args, true).await
}

async fn create_commit_syncers_from_app_impl<R: CrossRepo>(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_args: &SourceAndTargetRepoArgs,
    unredacted: bool,
) -> Result<Syncers<SqlSyncedCommitMapping, R>, Error> {
    let (source_repo, target_repo, mapping, live_commit_sync_config) =
        get_things_from_app::<R>(ctx, app, repo_args, unredacted).await?;

    let common_config =
        live_commit_sync_config.get_common_config(source_repo.0.repo_identity().id())?;

    let caching = app.environment().caching;
    let x_repo_syncer_lease = create_commit_syncer_lease(app.fb, caching)?;

    let repo_provider = repo_provider_from_mononoke_app(app);

    let submodule_deps = get_all_possible_small_repo_submodule_deps(
        source_repo.0.clone(),
        live_commit_sync_config.clone(),
        repo_provider,
    )
    .await?;

    let large_repo_id = common_config.large_repo_id;
    let source_repo_id = source_repo.0.repo_identity().id();
    let target_repo_id = target_repo.0.repo_identity().id();

    let (small_repo, large_repo) = if large_repo_id == source_repo_id {
        (target_repo.0, source_repo.0)
    } else if large_repo_id == target_repo_id {
        (source_repo.0, target_repo.0)
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
        mapping,
        live_commit_sync_config,
        x_repo_syncer_lease,
    )
}

/// Instantiate the source-target `CommitSyncer` struct by parsing `app`
pub async fn create_commit_syncer_from_app<R: CrossRepo>(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_args: &SourceAndTargetRepoArgs,
) -> Result<CommitSyncer<SqlSyncedCommitMapping, R>, Error> {
    create_commit_syncer_from_app_impl(ctx, app, false /* reverse */, repo_args).await
}

/// Instantiate the target-source `CommitSyncer` struct by parsing `app`
pub async fn create_reverse_commit_syncer_from_app<R: CrossRepo>(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_args: &SourceAndTargetRepoArgs,
) -> Result<CommitSyncer<SqlSyncedCommitMapping, R>, Error> {
    create_commit_syncer_from_app_impl(ctx, app, true /* reverse */, repo_args).await
}

/// Instantiate some auxiliary things from `app`
/// Naming is hard.
async fn get_things_from_app<R: CrossRepo>(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_args: &SourceAndTargetRepoArgs,
    unredacted: bool,
) -> Result<
    (
        Source<R>,
        Target<R>,
        SqlSyncedCommitMapping,
        Arc<dyn LiveCommitSyncConfig>,
    ),
    Error,
> {
    let fb = app.fb;

    let config_store = app.config_store();
    let (_, source_repo_config) = app.repo_config(repo_args.source_repo.as_repo_arg())?;
    let (_, target_repo_config) = app.repo_config(repo_args.target_repo.as_repo_arg())?;

    if source_repo_config.storage_config.metadata != target_repo_config.storage_config.metadata {
        return Err(Error::msg(
            "source repo and target repo have different metadata database configs!",
        ));
    }

    let mysql_options = app.mysql_options();
    let readonly_storage = app.readonly_storage();

    let mapping = SqlSyncedCommitMapping::with_metadata_database_config(
        fb,
        &source_repo_config.storage_config.metadata,
        mysql_options,
        readonly_storage.0,
    )
    .await?;

    let (source_repo, target_repo) = if !unredacted {
        app.open_source_and_target_repos(repo_args).await?
    } else {
        future::try_join(
            app.open_repo_unredacted(&repo_args.source_repo),
            app.open_repo_unredacted(&repo_args.target_repo),
        )
        .await?
    };

    let live_commit_sync_config: Arc<dyn LiveCommitSyncConfig> =
        Arc::new(CfgrLiveCommitSyncConfig::new(ctx.logger(), config_store)?);

    Ok((
        Source(source_repo),
        Target(target_repo),
        mapping,
        live_commit_sync_config,
    ))
}

fn flip_direction<T>(source_item: Source<T>, target_item: Target<T>) -> (Source<T>, Target<T>) {
    (Source(target_item.0), Target(source_item.0))
}

async fn create_commit_syncer_from_app_impl<R: CrossRepo>(
    ctx: &CoreContext,
    app: &MononokeApp,
    reverse: bool,
    repo_args: &SourceAndTargetRepoArgs,
) -> Result<CommitSyncer<SqlSyncedCommitMapping, R>, Error> {
    let (source_repo, target_repo, mapping, live_commit_sync_config) =
        get_things_from_app::<R>(ctx, app, repo_args, false).await?;

    let (source_repo, target_repo) = if reverse {
        flip_direction(source_repo, target_repo)
    } else {
        (source_repo, target_repo)
    };

    let caching = app.environment().caching;
    let x_repo_syncer_lease = create_commit_syncer_lease(app.fb, caching)?;

    let repo_provider = repo_provider_from_mononoke_app(app);

    let submodule_deps = get_all_possible_small_repo_submodule_deps(
        source_repo.0.clone(),
        live_commit_sync_config.clone(),
        repo_provider,
    )
    .await?;

    create_commit_syncer(
        ctx,
        source_repo,
        target_repo,
        submodule_deps,
        mapping,
        live_commit_sync_config,
        x_repo_syncer_lease,
    )
    .await
}

async fn create_commit_syncer<'a, R: CrossRepo>(
    ctx: &'a CoreContext,
    source_repo: Source<R>,
    target_repo: Target<R>,
    submodule_deps: SubmoduleDeps<R>,
    mapping: SqlSyncedCommitMapping,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    x_repo_syncer_lease: Arc<dyn LeaseOps>,
) -> Result<CommitSyncer<SqlSyncedCommitMapping, R>, Error> {
    let common_config =
        live_commit_sync_config.get_common_config(source_repo.0.repo_identity().id())?;

    let repos = CommitSyncRepos::new(source_repo.0, target_repo.0, submodule_deps, &common_config)?;
    let commit_syncer = CommitSyncer::new(
        ctx,
        mapping,
        repos,
        live_commit_sync_config,
        x_repo_syncer_lease,
    );
    Ok(commit_syncer)
}

/// Async function that, given a RepositoryId, loads and returns the repo.
pub type RepoProvider<'a, R> =
    Arc<dyn Fn(RepositoryId) -> Pin<Box<dyn Future<Output = Result<R>> + Send + 'a>> + 'a>;

pub fn repo_provider_from_mononoke_app<'a, R: CrossRepo>(
    app: &'a MononokeApp,
) -> RepoProvider<'a, R> {
    let repo_provider: RepoProvider<'a, R> = Arc::new(move |repo_id| {
        Box::pin(async move {
            let repo = app.open_repo_unredacted::<R>(&RepoArg::Id(repo_id)).await?;
            Ok(repo)
        })
    });

    repo_provider
}

/// Loads the Mononoke repos from the git submodules that the small repo depends.
///
/// These repos need to be loaded in order to be able to sync commits from the
/// small repo that have git submodule changes to the large repo.
///
/// Since the dependencies might change for each version, this eagerly loads
/// the dependencies from all versions, to guarantee that if we sync a sligthly
/// older commit, its dependencies will be loaded.
pub async fn get_all_possible_small_repo_submodule_deps<'a, R: CrossRepo>(
    small_repo: R,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    repo_provider: RepoProvider<'_, R>,
) -> Result<SubmoduleDeps<R>> {
    let source_repo_id = small_repo.repo_identity().id();

    let source_repo_sync_configs = live_commit_sync_config
        .get_all_commit_sync_config_versions(source_repo_id)
        .await?;

    let small_repo_deps_ids = source_repo_sync_configs
        .into_values()
        .filter_map(|mut cfg| {
            cfg.small_repos
                .remove(&source_repo_id)
                .map(|small_repo_cfg| small_repo_cfg.submodule_config.submodule_dependencies)
        })
        .flatten()
        .collect::<HashSet<_>>();

    let submodule_deps_map: HashMap<NonRootMPath, R> = stream::iter(small_repo_deps_ids)
        .then(|(submodule_path, repo_id)| {
            cloned!(repo_provider);
            async move {
                let repo = repo_provider(repo_id).await?;
                anyhow::Ok((submodule_path, repo))
            }
        })
        .try_collect()
        .await?;

    Ok(SubmoduleDeps::ForSync(submodule_deps_map))
}
