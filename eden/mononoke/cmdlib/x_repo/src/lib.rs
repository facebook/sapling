/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Scaffolding that's generally useful to build CLI tools on top of Mononoke.

#![deny(warnings)]

use anyhow::{bail, Error};
use blobrepo::BlobRepo;
use cmdlib::args::{self, MononokeMatches};
use context::CoreContext;
use cross_repo_sync::{
    create_commit_syncers,
    types::{Source, Target},
    CommitSyncRepos, CommitSyncer, Syncers,
};
use futures_util::try_join;
use live_commit_sync_config::{CfgrLiveCommitSyncConfig, LiveCommitSyncConfig};
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use std::sync::Arc;
use synced_commit_mapping::SqlSyncedCommitMapping;

/// Instantiate the `Syncers` struct by parsing `matches`
pub async fn create_commit_syncers_from_matches(
    ctx: &CoreContext,
    matches: &MononokeMatches<'_>,
) -> Result<Syncers<SqlSyncedCommitMapping>, Error> {
    let (source_repo, target_repo, mapping, live_commit_sync_config) =
        get_things_from_matches(ctx, matches).await?;

    let current_config = live_commit_sync_config
        .get_current_commit_sync_config(ctx, source_repo.0.get_repoid())
        .await?;

    let large_repo_id = current_config.large_repo_id;
    let source_repo_id = source_repo.0.get_repoid();
    let target_repo_id = target_repo.0.get_repoid();
    let (small_repo, large_repo) = if large_repo_id == source_repo_id {
        (target_repo.0, source_repo.0)
    } else if large_repo_id == target_repo_id {
        (source_repo.0, target_repo.0)
    } else {
        bail!(
            "Unexpectedly CommitSyncConfig {:?} has neither of {}, {} as a large repo",
            current_config,
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
    )
}

/// Instantiate the source-target `CommitSyncer` struct by parsing `matches`
pub async fn create_commit_syncer_from_matches(
    ctx: &CoreContext,
    matches: &MononokeMatches<'_>,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    create_commit_syncer_from_matches_impl(ctx, matches, false /* reverse */).await
}

/// Instantiate the target-source `CommitSyncer` struct by parsing `matches`
pub async fn create_reverse_commit_syncer_from_matches(
    ctx: &CoreContext,
    matches: &MononokeMatches<'_>,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    create_commit_syncer_from_matches_impl(ctx, matches, true /* reverse */).await
}

/// Instantiate some auxiliary things from `matches`
/// Naming is hard.
async fn get_things_from_matches(
    ctx: &CoreContext,
    matches: &MononokeMatches<'_>,
) -> Result<
    (
        Source<BlobRepo>,
        Target<BlobRepo>,
        SqlSyncedCommitMapping,
        Arc<dyn LiveCommitSyncConfig>,
    ),
    Error,
> {
    let fb = ctx.fb;
    let logger = ctx.logger();

    let config_store = args::init_config_store(fb, logger, matches)?;
    let source_repo_id = args::get_source_repo_id(config_store, &matches)?;
    let target_repo_id = args::get_target_repo_id(config_store, &matches)?;

    let (_, source_repo_config) =
        args::get_config_by_repoid(config_store, &matches, source_repo_id)?;
    let (_, target_repo_config) =
        args::get_config_by_repoid(config_store, &matches, target_repo_id)?;

    if source_repo_config.storage_config.metadata != target_repo_config.storage_config.metadata {
        return Err(Error::msg(
            "source repo and target repo have different metadata database configs!",
        ));
    }

    let mysql_options = args::parse_mysql_options(&matches);
    let readonly_storage = args::parse_readonly_storage(&matches);

    let mapping = SqlSyncedCommitMapping::with_metadata_database_config(
        ctx.fb,
        &source_repo_config.storage_config.metadata,
        &mysql_options,
        readonly_storage.0,
    )
    .await?;

    let source_repo_fut = args::open_repo_with_repo_id(fb, logger, source_repo_id, &matches);
    let target_repo_fut = args::open_repo_with_repo_id(fb, logger, target_repo_id, &matches);

    let (source_repo, target_repo) = try_join!(source_repo_fut, target_repo_fut)?;

    let live_commit_sync_config: Arc<dyn LiveCommitSyncConfig> =
        Arc::new(CfgrLiveCommitSyncConfig::new(&ctx.logger(), config_store)?);

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

async fn create_commit_syncer_from_matches_impl(
    ctx: &CoreContext,
    matches: &MononokeMatches<'_>,
    reverse: bool,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    let (source_repo, target_repo, mapping, live_commit_sync_config) =
        get_things_from_matches(ctx, matches).await?;

    let (source_repo, target_repo) = if reverse {
        flip_direction(source_repo, target_repo)
    } else {
        (source_repo, target_repo)
    };

    create_commit_syncer(
        ctx,
        source_repo,
        target_repo,
        mapping,
        live_commit_sync_config,
    )
    .await
}

async fn create_commit_syncer<'a>(
    ctx: &'a CoreContext,
    source_repo: Source<BlobRepo>,
    target_repo: Target<BlobRepo>,
    mapping: SqlSyncedCommitMapping,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    let current_config = live_commit_sync_config
        .get_current_commit_sync_config(ctx, source_repo.0.get_repoid())
        .await?;

    let repos = CommitSyncRepos::new(source_repo.0, target_repo.0, &current_config)?;
    let commit_syncer = CommitSyncer::new(ctx, mapping, repos, live_commit_sync_config);
    Ok(commit_syncer)
}
