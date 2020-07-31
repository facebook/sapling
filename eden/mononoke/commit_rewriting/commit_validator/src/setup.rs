/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Context, Error, Result};
use blobrepo::BlobRepo;
use blobrepo_factory::ReadOnlyStorage;
use bookmarks::BookmarkName;
use clap::ArgMatches;
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use cross_repo_sync::types::{Large, Small};
use fbinit::FacebookInit;
use futures::future::try_join_all;
use futures::{compat::Future01CompatExt, future::TryFutureExt};
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use metaconfig_types::RepoConfig;
use mononoke_types::RepositoryId;
use mutable_counters::MutableCounters;
use reachabilityindex::LeastCommonAncestorsHint;
use scuba_ext::ScubaSampleBuilder;
use skiplist::fetch_skiplist_index;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::facebook::MysqlOptions;
use std::sync::Arc;
use synced_commit_mapping::SqlSyncedCommitMapping;

use crate::cli::{ARG_ENTRY_ID, ARG_MASTER_BOOKMARK, ARG_START_ID};
use crate::reporting::add_common_commit_syncing_fields;
use crate::validation::ValidationHelpers;

pub async fn get_validation_helpers(
    fb: FacebookInit,
    ctx: CoreContext,
    large_repo: BlobRepo,
    repo_config: RepoConfig,
    matches: ArgMatches<'static>,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    scuba_sample: ScubaSampleBuilder,
) -> Result<ValidationHelpers, Error> {
    let repo_id = large_repo.get_repoid();

    let config_store = args::maybe_init_config_store(fb, ctx.logger(), &matches)
        .ok_or_else(|| format_err!("Failed to init ConfigStore."))?;
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(ctx.logger(), &config_store)?;

    let commit_sync_config = repo_config.commit_sync_config.clone().ok_or(format_err!(
        "CommitSyncConfig not available for repo {}",
        repo_id
    ))?;

    if repo_id != commit_sync_config.large_repo_id {
        return Err(format_err!(
            "Validator job must run on the large repo. {} is not large!",
            repo_id
        ));
    }

    let mapping = SqlSyncedCommitMapping::with_metadata_database_config(
        fb,
        &repo_config.storage_config.metadata,
        mysql_options,
        readonly_storage.0,
    )
    .await?;

    let large_repo_lca_hint = get_lca_hint(ctx.clone(), &large_repo, &repo_config)
        .await
        .context("While creating lca_hint")?;

    let large_repo_master_bookmark = get_master_bookmark(&matches)?;

    let validation_helper_futs =
        commit_sync_config
            .small_repos
            .into_iter()
            .map(|(small_repo_id, _)| {
                let scuba_sample = {
                    let mut scuba_sample = scuba_sample.clone();
                    add_common_commit_syncing_fields(
                        &mut scuba_sample,
                        Large(large_repo.get_repoid()),
                        Small(small_repo_id),
                    );

                    scuba_sample
                };

                args::open_repo_with_repo_id(fb, ctx.logger(), small_repo_id, &matches)
                    .compat()
                    .and_then({
                        cloned!(large_repo);
                        move |small_repo| async move {
                            Ok((
                                small_repo_id,
                                (Large(large_repo), Small(small_repo), scuba_sample),
                            ))
                        }
                    })
            });

    let validation_helpers = try_join_all(validation_helper_futs).await?;

    Ok(ValidationHelpers::new(
        large_repo,
        validation_helpers.into_iter().collect(),
        large_repo_lca_hint,
        large_repo_master_bookmark,
        mapping,
        live_commit_sync_config,
    ))
}

pub fn format_counter() -> String {
    "x_repo_commit_validator".to_string()
}

pub async fn get_start_id<T: MutableCounters>(
    ctx: CoreContext,
    repo_id: RepositoryId,
    mutable_counters: &T,
    matches: ArgMatches<'static>,
) -> Result<u64, Error> {
    match matches.value_of(ARG_START_ID) {
        Some(start_id) => start_id
            .parse::<u64>()
            .map_err(|_| format_err!("{} must be a valid u64", ARG_START_ID)),
        None => {
            let counter = format_counter();
            mutable_counters
                .get_counter(ctx.clone(), repo_id, &counter)
                .compat()
                .await?
                .ok_or(format_err!("mutable counter {} is missing", counter))
                .map(|val| val as u64)
        }
    }
}

pub fn get_entry_id(matches: ArgMatches<'static>) -> Result<u64, Error> {
    matches
        .value_of(ARG_ENTRY_ID)
        .ok_or(format_err!("Entry id argument missing"))?
        .parse::<u64>()
        .map_err(|_| format_err!("{} must be a valid u64", ARG_ENTRY_ID))
}

async fn get_lca_hint(
    ctx: CoreContext,
    large_repo: &BlobRepo,
    large_repo_config: &RepoConfig,
) -> Result<Arc<dyn LeastCommonAncestorsHint>, Error> {
    let lca_hint: Arc<dyn LeastCommonAncestorsHint> = fetch_skiplist_index(
        &ctx,
        &large_repo_config.skiplist_index_blobstore_key,
        &large_repo.get_blobstore().boxed(),
    )
    .await?;

    Ok(lca_hint)
}

fn get_master_bookmark<'a, 'b>(matches: &'a ArgMatches<'b>) -> Result<BookmarkName, Error> {
    let name = matches
        .value_of(ARG_MASTER_BOOKMARK)
        .ok_or(format_err!("Argument {} is required", ARG_MASTER_BOOKMARK))?;
    BookmarkName::new(name)
}
