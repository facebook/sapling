/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use blobstore_factory::ReadOnlyStorage;
use bookmarks::BookmarkKey;
use borrowed::borrowed;
use context::CoreContext;
use cross_repo_sync::types::Large;
use cross_repo_sync::types::Small;
use fbinit::FacebookInit;
use futures::future::try_join_all;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use live_commit_sync_config::LiveCommitSyncConfig;
use metaconfig_types::RepoConfig;
use mononoke_api_types::InnerRepo;
use mononoke_app::args::RepoArg;
use mononoke_app::MononokeApp;
use mutable_counters::MutableCountersRef;
use repo_identity::RepoIdentityRef;
use scuba_ext::MononokeScubaSampleBuilder;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::facebook::MysqlOptions;
use synced_commit_mapping::SqlSyncedCommitMapping;

use crate::cli::MononokeCommitValidatorArgs;
use crate::reporting::add_common_commit_syncing_fields;
use crate::validation::ValidationHelpers;

pub async fn get_validation_helpers<'a>(
    fb: FacebookInit,
    ctx: CoreContext,
    app: &MononokeApp,
    large_repo: InnerRepo,
    repo_config: RepoConfig,
    mysql_options: MysqlOptions,
    readonly_storage: ReadOnlyStorage,
    scuba_sample: MononokeScubaSampleBuilder,
) -> Result<ValidationHelpers, Error> {
    let repo_id = large_repo.blob_repo.repo_identity().id();

    let config_store = app.config_store();
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(ctx.logger(), config_store)?;
    let common_commit_sync_config = live_commit_sync_config.get_common_config(repo_id)?;

    let mapping = SqlSyncedCommitMapping::with_metadata_database_config(
        fb,
        &repo_config.storage_config.metadata,
        &mysql_options,
        readonly_storage.0,
    )
    .await?;

    let large_repo_master_bookmark =
        BookmarkKey::new(app.args::<MononokeCommitValidatorArgs>()?.master_bookmark)?;

    let validation_helper_futs =
        common_commit_sync_config
            .small_repos
            .into_keys()
            .map(|small_repo_id| {
                let large_blob_repo = large_repo.blob_repo.clone();
                borrowed!(app, scuba_sample);
                async move {
                    let scuba_sample = {
                        let mut scuba_sample = scuba_sample.clone();
                        add_common_commit_syncing_fields(
                            &mut scuba_sample,
                            Large(large_blob_repo.repo_identity().id()),
                            Small(small_repo_id),
                        );

                        scuba_sample
                    };
                    let small_repo = app.open_repo(&RepoArg::Id(small_repo_id)).await?;
                    Result::<_, Error>::Ok((
                        small_repo_id,
                        (Large(large_blob_repo), Small(small_repo), scuba_sample),
                    ))
                }
            });

    let validation_helpers = try_join_all(validation_helper_futs).await?;

    Ok(ValidationHelpers::new(
        large_repo,
        validation_helpers.into_iter().collect(),
        large_repo_master_bookmark,
        mapping,
        live_commit_sync_config,
    ))
}

pub fn format_counter() -> String {
    "x_repo_commit_validator".to_string()
}

pub async fn get_start_id<'a>(
    ctx: &CoreContext,
    repo: &impl MutableCountersRef,
    start_id: Option<u64>,
) -> Result<u64, Error> {
    match start_id {
        Some(start_id) => Ok(start_id),
        None => {
            let counter = format_counter();
            repo.mutable_counters()
                .get_counter(ctx, &counter)
                .await?
                .ok_or_else(|| format_err!("mutable counter {} is missing", counter))
                .map(|val| val as u64)
        }
    }
}
