/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use blobstore_factory::make_metadata_sql_factory;
use blobstore_factory::ReadOnlyStorage;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::Bookmarks;
use clap_old::App;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use context::CoreContext;
use fbinit::FacebookInit;
use metaconfig_types::MetadataDatabaseConfig;
use repo_blobstore::RepoBlobstore;
use repo_identity::RepoIdentity;
use segmented_changelog::copy_segmented_changelog;
use segmented_changelog::SegmentedChangelogSqlConnections;
use slog::Logger;
use sql_ext::facebook::MyAdmin;
use sql_ext::replication::NoReplicaLagMonitor;
use sql_ext::replication::ReplicaLagMonitor;
use std::sync::Arc;

use crate::error::SubcommandError;

pub const TRUNCATE_SEGMENTED_CHANGELOG: &str = "truncate-segmented-changelog";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(TRUNCATE_SEGMENTED_CHANGELOG)
        .about("rewinds segmented changelog to remove newer commits")
        .args_from_usage(
            r#"<CHANGESET_ID>    'changeset ID to truncate to. You will need to tail from here to recover'"#
        )
}

pub async fn subcommand_truncate_segmented_changelog<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let rev = sub_m.value_of("CHANGESET_ID").unwrap().to_string();

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    #[facet::container]
    struct CopySegmentedChangelogContainer {
        #[facet]
        id: RepoIdentity,
        #[facet]
        blobstore: RepoBlobstore,
        // For commit lookup - not needed by the copy operation
        #[facet]
        hg_mapping: dyn BonsaiHgMapping,
        #[facet]
        bookmarks: dyn Bookmarks,
    }
    let container: CopySegmentedChangelogContainer = args::open_repo(fb, &logger, matches).await?;

    let config_store = matches.config_store();
    let mysql_options = matches.mysql_options();
    let (_, config) = args::get_config(config_store, matches)?;
    let storage_config = config.storage_config;
    let readonly_storage = ReadOnlyStorage(false);

    let db_address = match &storage_config.metadata {
        MetadataDatabaseConfig::Local(_) => None,
        MetadataDatabaseConfig::Remote(remote_config) => {
            Some(remote_config.primary.db_address.clone())
        }
    };
    let replica_lag_monitor: Arc<dyn ReplicaLagMonitor> = match db_address {
        None => Arc::new(NoReplicaLagMonitor()),
        Some(address) => {
            let my_admin = MyAdmin::new(ctx.fb).context("building myadmin client")?;
            Arc::new(my_admin.single_shard_lag_monitor(address))
        }
    };

    let sql_factory = make_metadata_sql_factory(
        ctx.fb,
        storage_config.metadata,
        mysql_options.clone(),
        readonly_storage,
    )
    .await
    .context("constructing metadata sql factory")?;

    let segmented_changelog_sql_connections = sql_factory
        .open::<SegmentedChangelogSqlConnections>()
        .context("error opening segmented changelog sql connections")?;

    let heads = vec![helpers::csid_resolve(&ctx, &container, rev).await?];
    copy_segmented_changelog(
        &ctx,
        container.id.id(),
        segmented_changelog_sql_connections,
        container.blobstore,
        replica_lag_monitor,
        heads,
    )
    .await
    .context("While truncating")?;
    Ok(())
}
