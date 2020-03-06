/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Scaffolding that's generally useful to build CLI tools on top of Mononoke.

#![deny(warnings)]

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobrepo_factory::ReadOnlyStorage;
use clap::ArgMatches;
use cmdlib::{args, helpers::open_sql_with_config_and_mysql_options};
use cross_repo_sync::{CommitSyncRepos, CommitSyncer};
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use futures_util::try_join;
use metaconfig_types::RepoConfig;
use slog::Logger;
use sql_ext::facebook::MysqlOptions;
use synced_commit_mapping::SqlSyncedCommitMapping;

// Creates commits syncer from source to target
pub async fn create_commit_syncer_from_matches<'a>(
    fb: FacebookInit,
    logger: &Logger,
    matches: &ArgMatches<'a>,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    create_commit_syncer_from_matches_impl(fb, logger, matches, false /*reverse*/).await
}

// Creates commit syncer from target to source
pub async fn create_reverse_commit_syncer_from_matches<'a>(
    fb: FacebookInit,
    logger: &Logger,
    matches: &ArgMatches<'a>,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    create_commit_syncer_from_matches_impl(fb, logger, matches, true /*reverse*/).await
}

async fn create_commit_syncer_from_matches_impl<'a>(
    fb: FacebookInit,
    logger: &Logger,
    matches: &ArgMatches<'a>,
    reverse: bool,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    let source_repo_id = args::get_source_repo_id(fb, &matches)?;
    let target_repo_id = args::get_target_repo_id(fb, &matches)?;

    let (_, source_repo_config) = args::get_config_by_repoid(fb, &matches, source_repo_id)?;
    let (_, target_repo_config) = args::get_config_by_repoid(fb, &matches, target_repo_id)?;
    let source_repo_fut = args::open_repo_with_repo_id(fb, logger, source_repo_id, &matches);
    let target_repo_fut = args::open_repo_with_repo_id(fb, logger, target_repo_id, &matches);

    let (source_repo, target_repo) = try_join!(source_repo_fut.compat(), target_repo_fut.compat())?;

    let mysql_options = args::parse_mysql_options(&matches);
    let readonly_storage = args::parse_readonly_storage(&matches);

    if reverse {
        create_commit_syncer(
            fb,
            (target_repo, target_repo_config),
            (source_repo, source_repo_config),
            mysql_options,
            readonly_storage,
        )
        .await
    } else {
        create_commit_syncer(
            fb,
            (source_repo, source_repo_config),
            (target_repo, target_repo_config),
            mysql_options,
            readonly_storage,
        )
        .await
    }
}

async fn create_commit_syncer<'a>(
    fb: FacebookInit,
    (source_repo, source_config): (BlobRepo, RepoConfig),
    (target_repo, target_config): (BlobRepo, RepoConfig),
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
    if source_config.storage_config.dbconfig != target_config.storage_config.dbconfig {
        return Err(Error::msg(
            "source repo and target repo have different db configs!",
        ));
    }

    let mapping = open_sql_with_config_and_mysql_options::<SqlSyncedCommitMapping>(
        fb,
        source_config.storage_config.dbconfig.clone(),
        mysql_options,
        readonly_storage,
    )
    .compat()
    .await?;

    let commit_sync_config = source_config
        .commit_sync_config
        .as_ref()
        .ok_or_else(|| format_err!("missing CommitSyncMapping config"))?;

    let commit_sync_repos = CommitSyncRepos::new(source_repo, target_repo, &commit_sync_config)?;
    Ok(CommitSyncer::new(mapping, commit_sync_repos))
}
