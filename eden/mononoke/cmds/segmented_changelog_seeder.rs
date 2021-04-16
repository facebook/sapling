/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::sync::Arc;

use anyhow::{Context, Error};
use clap::Arg;
use futures::compat::Future01CompatExt;
use slog::info;

use blobstore_factory::{make_metadata_sql_factory, ReadOnlyStorage};
use cmdlib::{
    args::{self, MononokeMatches},
    helpers,
};
use context::CoreContext;
use fbinit::FacebookInit;
use metaconfig_types::MetadataDatabaseConfig;
use segmented_changelog::types::IdMapVersion;
use segmented_changelog::{SegmentedChangelogSeeder, SegmentedChangelogSqlConnections};
use sql_ext::facebook::MyAdmin;
use sql_ext::replication::{NoReplicaLagMonitor, ReplicaLagMonitor};

const IDMAP_VERSION_ARG: &str = "idmap-version";
const HEAD_ARG: &str = "head";

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeAppBuilder::new("Builds a new version of segmented changelog.")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .build()
        .about("Builds a new version of segmented changelog.")
        .arg(
            Arg::with_name(IDMAP_VERSION_ARG)
                .long(IDMAP_VERSION_ARG)
                .takes_value(true)
                .required(false)
                .help("What version to label the new idmap with."),
        )
        .arg(
            Arg::with_name(HEAD_ARG)
                .long(HEAD_ARG)
                .takes_value(true)
                .help("What head to use for Segmented Changelog."),
        );
    let matches = app.get_matches(fb)?;

    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    helpers::block_execute(
        run(ctx, &matches),
        fb,
        &std::env::var("TW_JOB_NAME").unwrap_or_else(|_| "segmented_changelog_seeder".to_string()),
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}

async fn run<'a>(ctx: CoreContext, matches: &'a MononokeMatches<'a>) -> Result<(), Error> {
    let idmap_version_arg: Option<u64> = args::get_u64_opt(matches, IDMAP_VERSION_ARG);
    let config_store = matches.config_store();

    // This is a bit weird from the dependency point of view but I think that it is best. The
    // BlobRepo may have a SegmentedChangelog attached to it but that doesn't hurt us in any way.
    // On the other hand reconstructing the dependencies for SegmentedChangelog without BlobRepo is
    // probably prone to more problems from the maintenance perspective.
    let repo = args::open_repo(ctx.fb, ctx.logger(), &matches)
        .await
        .context("opening repo")?;

    let mysql_options = args::parse_mysql_options(matches);
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
        mysql_options,
        readonly_storage,
        ctx.logger(),
    )
    .await
    .context("constructing metadata sql factory")?;

    let segmented_changelog_sql_connections = sql_factory
        .open::<SegmentedChangelogSqlConnections>()
        .await
        .context("error opening segmented changelog sql connections")?;

    let segmented_changelog_seeder = SegmentedChangelogSeeder::new(
        repo.get_repoid(),
        segmented_changelog_sql_connections,
        replica_lag_monitor,
        repo.get_changesets_object(),
        repo.get_phases(),
        Arc::new(repo.get_blobstore()),
    );

    info!(
        ctx.logger(),
        "SegmentedChangelogSeeder initialized for repository '{}'",
        repo.name()
    );

    let head_arg = matches
        .value_of(HEAD_ARG)
        .unwrap_or(&config.segmented_changelog_config.master_bookmark);
    let head = helpers::csid_resolve(ctx.clone(), repo.clone(), head_arg)
        .compat()
        .await
        .with_context(|| format!("resolving head csid for '{}'", head_arg))?;
    info!(ctx.logger(), "using '{}' for head", head);

    if let Some(idmap_version) = idmap_version_arg {
        segmented_changelog_seeder
            .run_with_idmap_version(&ctx, head, IdMapVersion(idmap_version))
            .await
            .context("seeding segmented changelog")?;
    } else {
        segmented_changelog_seeder
            .run(&ctx, head)
            .await
            .context("seeding segmented changelog")?;
    }

    info!(
        ctx.logger(),
        "successfully finished seeding SegmentedChangelog for repository '{}'",
        repo.name(),
    );

    Ok(())
}
