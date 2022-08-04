/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Scaffolding that's generally useful to build CLI tools on top of Mononoke.

use std::sync::Arc;

use anyhow::bail;
use anyhow::Error;
use blobrepo::BlobRepo;
use cacheblob::LeaseOps;
use context::CoreContext;
use cross_repo_sync::create_commit_syncer_lease;
use cross_repo_sync::create_commit_syncers;
use cross_repo_sync::types::Source;
use cross_repo_sync::types::Target;
use cross_repo_sync::CommitSyncRepos;
use cross_repo_sync::CommitSyncer;
use cross_repo_sync::Syncers;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use live_commit_sync_config::LiveCommitSyncConfig;
use mononoke_app::args::SourceAndTargetRepoArgs;
use mononoke_app::MononokeApp;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use synced_commit_mapping::SqlSyncedCommitMapping;

/// Instantiate the `Syncers` struct by parsing `app`
pub async fn create_commit_syncers_from_app(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_args: &SourceAndTargetRepoArgs,
) -> Result<Syncers<SqlSyncedCommitMapping>, Error> {
    let (source_repo, target_repo, mapping, live_commit_sync_config) =
        get_things_from_app(ctx, app, repo_args).await?;

    let common_config = live_commit_sync_config.get_common_config(source_repo.0.get_repoid())?;

    let caching = app.environment().caching;
    let x_repo_syncer_lease = create_commit_syncer_lease(app.fb, caching)?;

    let large_repo_id = common_config.large_repo_id;
    let source_repo_id = source_repo.0.get_repoid();
    let target_repo_id = target_repo.0.get_repoid();
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
        mapping,
        live_commit_sync_config,
        x_repo_syncer_lease,
    )
}

/// Instantiate the source-target `CommitSyncer` struct by parsing `app`
pub async fn create_commit_syncer_from_app(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_args: &SourceAndTargetRepoArgs,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    create_commit_syncer_from_app_impl(ctx, app, false /* reverse */, repo_args).await
}

/// Instantiate the target-source `CommitSyncer` struct by parsing `app`
pub async fn create_reverse_commit_syncer_from_app(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_args: &SourceAndTargetRepoArgs,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    create_commit_syncer_from_app_impl(ctx, app, true /* reverse */, repo_args).await
}

/// Instantiate some auxiliary things from `app`
/// Naming is hard.
async fn get_things_from_app(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo_args: &SourceAndTargetRepoArgs,
) -> Result<
    (
        Source<BlobRepo>,
        Target<BlobRepo>,
        SqlSyncedCommitMapping,
        Arc<dyn LiveCommitSyncConfig>,
    ),
    Error,
> {
    let fb = app.fb;

    let config_store = app.config_store();
    let ((_, source_repo_config), (_, target_repo_config)) =
        app.source_and_target_repo_config(repo_args.source_and_target_id_or_name()?)?;

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
    )?;

    let (source_repo, target_repo) = app.open_source_and_target_repos(repo_args).await?;

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

async fn create_commit_syncer_from_app_impl(
    ctx: &CoreContext,
    app: &MononokeApp,
    reverse: bool,
    repo_args: &SourceAndTargetRepoArgs,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    let (source_repo, target_repo, mapping, live_commit_sync_config) =
        get_things_from_app(ctx, app, repo_args).await?;

    let (source_repo, target_repo) = if reverse {
        flip_direction(source_repo, target_repo)
    } else {
        (source_repo, target_repo)
    };

    let caching = app.environment().caching;
    let x_repo_syncer_lease = create_commit_syncer_lease(app.fb, caching)?;

    create_commit_syncer(
        ctx,
        source_repo,
        target_repo,
        mapping,
        live_commit_sync_config,
        x_repo_syncer_lease,
    )
    .await
}

async fn create_commit_syncer<'a>(
    ctx: &'a CoreContext,
    source_repo: Source<BlobRepo>,
    target_repo: Target<BlobRepo>,
    mapping: SqlSyncedCommitMapping,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    x_repo_syncer_lease: Arc<dyn LeaseOps>,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    let common_config = live_commit_sync_config.get_common_config(source_repo.0.get_repoid())?;

    let repos = CommitSyncRepos::new(source_repo.0, target_repo.0, &common_config)?;
    let commit_syncer = CommitSyncer::new(
        ctx,
        mapping,
        repos,
        live_commit_sync_config,
        x_repo_syncer_lease,
    );
    Ok(commit_syncer)
}
