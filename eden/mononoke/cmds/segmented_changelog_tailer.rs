/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Error};
use clap::{Arg, ArgMatches};
use futures::compat::Future01CompatExt;
use slog::info;

use blobstore_factory::{make_metadata_sql_factory, ReadOnlyStorage};
use bookmarks::BookmarkName;
use cmdlib::{args, helpers};
use context::CoreContext;
use fbinit::FacebookInit;
use metaconfig_types::MetadataDatabaseConfig;
use segmented_changelog::SegmentedChangelogBuilder;
use sql_ext::facebook::MyAdmin;
use sql_ext::replication::{NoReplicaLagMonitor, ReplicaLagMonitor};

const DELAY_ARG: &str = "delay";
const ONCE_ARG: &str = "once";
const TRACK_BOOKMARK_ARG: &str = "track-bookmark";

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeApp::new("Updates segmented changelog assets.")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .build()
        .version("0.0.0")
        .about("Builds a new version of segmented changelog.")
        .arg(
            Arg::with_name(DELAY_ARG)
                .long(DELAY_ARG)
                .takes_value(true)
                .required(false)
                .help("Delay period in seconds between incremental build runs."),
        )
        .arg(
            Arg::with_name(ONCE_ARG)
                .long(ONCE_ARG)
                .takes_value(false)
                .required(false)
                .help("When set, the tailer will perform a single incremental build run."),
        )
        .arg(
            Arg::with_name(TRACK_BOOKMARK_ARG)
                .long(TRACK_BOOKMARK_ARG)
                .takes_value(true)
                .required(false)
                .help("What bookmark to use as the head of the Segmented Changelog."),
        );
    let matches = app.get_matches();

    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    helpers::block_execute(
        run(ctx, &matches),
        fb,
        &std::env::var("TW_JOB_NAME").unwrap_or_else(|_| "segmented_changelog_tailer".to_string()),
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}

async fn run<'a>(ctx: CoreContext, matches: &'a ArgMatches<'a>) -> Result<(), Error> {
    // This is a bit weird from the dependency point of view but I think that it is best. The
    // BlobRepo may have a SegmentedChangelog attached to it but that doesn't hurt us in any way.
    // On the other hand reconstructing the dependencies for SegmentedChangelog without BlobRepo is
    // probably prone to more problems from the maintenance perspective.
    let repo = args::open_repo(ctx.fb, ctx.logger(), &matches)
        .compat()
        .await
        .context("opening repo")?;

    let mysql_options = args::parse_mysql_options(matches);
    let config_store = args::init_config_store(ctx.fb, ctx.logger(), matches)?;
    let (_, config) = args::get_config(config_store, &matches)?;
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
        ctx.logger().clone(),
    )
    .compat()
    .await
    .context("constructing metadata sql factory")?;

    let track_bookmark =
        BookmarkName::new(matches.value_of(TRACK_BOOKMARK_ARG).unwrap_or("master"))
            .context("parsing the name of the bookmark to track")?;

    let segmented_changelog_builder = sql_factory
        .open::<SegmentedChangelogBuilder>()
        .compat()
        .await
        .context("constructing segmented changelog builder")?;

    let segmented_changelog_tailer = segmented_changelog_builder
        .with_repo_id(repo.get_repoid())
        .with_replica_lag_monitor(replica_lag_monitor)
        .with_changeset_fetcher(repo.get_changeset_fetcher())
        .with_bookmarks(repo.bookmarks())
        .with_bookmark_name(track_bookmark)
        .with_blobstore(Arc::new(repo.get_blobstore()))
        .build_tailer()
        .context("building SegmentedChangelogTailer")?;

    info!(
        ctx.logger(),
        "SegmentedChangelogTailer initialized for repository '{}'",
        repo.name()
    );

    if matches.is_present(ONCE_ARG) {
        segmented_changelog_tailer
            .once(&ctx)
            .await
            .with_context(|| format!("incrementally building repo {}", repo.name()))?;
    } else {
        let delay = Duration::from_secs(args::get_u64(&matches, DELAY_ARG, 60));
        segmented_changelog_tailer
            .run(&ctx, delay)
            .await
            .with_context(|| format!("continuously building repo {}", repo.name()))?;
    }

    info!(
        ctx.logger(),
        "SegmentedChangelogTailer is done for repo {}",
        repo.name(),
    );

    Ok(())
}
