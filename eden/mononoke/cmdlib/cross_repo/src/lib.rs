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
use blobstore_factory::MetadataSqlFactory;
use context::CoreContext;
use cross_repo_sync::create_commit_syncer_lease;
use cross_repo_sync::create_commit_syncers;
use cross_repo_sync::get_all_submodule_deps;
use cross_repo_sync::RepoProvider;
use cross_repo_sync::Source;
use cross_repo_sync::Syncers;
use cross_repo_sync::Target;
use futures::future;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use live_commit_sync_config::LiveCommitSyncConfig;
use mononoke_app::args::AsRepoArg;
use mononoke_app::args::RepoArg;
use mononoke_app::args::SourceAndTargetRepoArgs;
use mononoke_app::MononokeApp;
use pushredirect::SqlPushRedirectionConfigBuilder;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_query_config::SqlQueryConfigArc;
use synced_commit_mapping::SqlSyncedCommitMapping;

pub trait CrossRepo = cross_repo_sync::Repo
    + SqlQueryConfigArc
    + for<'a> facet::AsyncBuildable<'a, repo_factory::RepoFactoryBuilder<'a>>;

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

    let source_repo_arc = Arc::new(source_repo.0.clone());
    let target_repo_arc = Arc::new(target_repo.0.clone());

    let submodule_deps = get_all_submodule_deps(
        ctx,
        source_repo_arc,
        target_repo_arc,
        repo_provider,
        live_commit_sync_config.clone(),
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

/// Instantiate some auxiliary things from `app`
/// Naming is hard.
async fn get_things_from_app<R: CrossRepo>(
    _ctx: &CoreContext,
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

    let (source_repo, target_repo): (R, R) = if !unredacted {
        app.open_source_and_target_repos(repo_args).await?
    } else {
        future::try_join(
            app.open_repo_unredacted(&repo_args.source_repo),
            app.open_repo_unredacted(&repo_args.target_repo),
        )
        .await?
    };

    let sql_factory: MetadataSqlFactory = MetadataSqlFactory::new(
        app.fb,
        source_repo_config.storage_config.metadata,
        mysql_options.clone(),
        blobstore_factory::ReadOnlyStorage(readonly_storage.0),
    )
    .await?;
    let builder = sql_factory
        .open::<SqlPushRedirectionConfigBuilder>()
        .await?;
    let push_redirection_config = builder.build(source_repo.sql_query_config_arc());

    let live_commit_sync_config: Arc<dyn LiveCommitSyncConfig> = Arc::new(
        CfgrLiveCommitSyncConfig::new(config_store, Arc::new(push_redirection_config))?,
    );

    Ok((
        Source(source_repo),
        Target(target_repo),
        mapping,
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
