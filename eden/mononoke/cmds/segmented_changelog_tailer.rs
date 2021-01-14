/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::sync::Arc;
use std::time::Duration;

use anyhow::{format_err, Context, Error};
use clap::Arg;
use futures::compat::Future01CompatExt;
use futures::future::join_all;
use slog::{error, info};

use blobstore_factory::{make_metadata_sql_factory, ReadOnlyStorage};
use bookmarks::BookmarkName;
use cmdlib::{
    args::{self, MononokeMatches},
    helpers,
};
use context::CoreContext;
use fbinit::FacebookInit;
use metaconfig_types::MetadataDatabaseConfig;
use segmented_changelog::SegmentedChangelogBuilder;
use sql_ext::facebook::MyAdmin;
use sql_ext::replication::{NoReplicaLagMonitor, ReplicaLagMonitor};

const DELAY_ARG: &str = "delay";
const ONCE_ARG: &str = "once";
const REPO_ARG: &str = "repo";
const TRACK_BOOKMARK_ARG: &str = "track-bookmark";

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeAppBuilder::new("Updates segmented changelog assets.")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .build()
        .about("Builds a new version of segmented changelog.")
        .arg(
            Arg::with_name(REPO_ARG)
                .long(REPO_ARG)
                .takes_value(true)
                .required(true)
                .multiple(true)
                .help("Repository name to warm-up"),
        )
        .arg(
            // it would make sense to pair the delay with the repository
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
            // it would make sense to pair the bookmark with the repository
            Arg::with_name(TRACK_BOOKMARK_ARG)
                .long(TRACK_BOOKMARK_ARG)
                .takes_value(true)
                .required(false)
                .help("What bookmark to use as the head of the Segmented Changelog."),
        );
    let matches = app.get_matches();

    let logger = args::init_logging(fb, &matches)?;
    args::init_cachelib(fb, &matches);
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

async fn run<'a>(ctx: CoreContext, matches: &'a MononokeMatches<'a>) -> Result<(), Error> {
    let reponames: Vec<_> = matches
        .values_of(REPO_ARG)
        .ok_or_else(|| format_err!("--{} argument is required", REPO_ARG))?
        .map(ToString::to_string)
        .collect();
    if reponames.is_empty() {
        error!(ctx.logger(), "At least one repo had to be specified");
        return Ok(());
    }

    let config_store = args::init_config_store(ctx.fb, ctx.logger(), matches)?;
    let mysql_options = args::parse_mysql_options(matches);
    let configs = args::load_repo_configs(config_store, matches)?;
    let readonly_storage = ReadOnlyStorage(false);

    let track_bookmark =
        BookmarkName::new(matches.value_of(TRACK_BOOKMARK_ARG).unwrap_or("master"))
            .context("parsing the name of the bookmark to track")?;

    let mut tasks = Vec::new();
    let repo_count = reponames.len() as u32;
    for (index, reponame) in reponames.into_iter().enumerate() {
        let config = configs
            .repos
            .get(&reponame)
            .ok_or_else(|| format_err!("unknown repository: {}", reponame))?;
        let repo_id = config.repoid;
        info!(
            ctx.logger(),
            "repo name '{}' translates to id {}", reponame, repo_id
        );

        let storage_config = config.storage_config.clone();
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
            ctx.logger().clone(),
        )
        .compat()
        .await
        .with_context(|| format!("repo {}: constructing metadata sql factory", repo_id))?;

        let segmented_changelog_builder = sql_factory
            .open::<SegmentedChangelogBuilder>()
            .compat()
            .await
            .with_context(|| {
                format!("repo {}: constructing segmented changelog builder", repo_id)
            })?;

        // This is a bit weird from the dependency point of view but I think that it is best. The
        // BlobRepo may have a SegmentedChangelog attached to it but that doesn't hurt us in any
        // way.  On the other hand reconstructing the dependencies for SegmentedChangelog without
        // BlobRepo is probably prone to more problems from the maintenance perspective.
        let blobrepo = args::open_repo_with_repo_id(ctx.fb, ctx.logger(), repo_id, matches).await?;
        let segmented_changelog_tailer = segmented_changelog_builder
            .with_blobrepo(&blobrepo)
            .with_replica_lag_monitor(replica_lag_monitor)
            .with_bookmark_name(track_bookmark.clone())
            .build_tailer()
            .with_context(|| format!("repo {}: building SegmentedChangelogTailer", repo_id))?;

        info!(
            ctx.logger(),
            "repo {}: SegmentedChangelogTailer initialized", repo_id
        );

        if matches.is_present(ONCE_ARG) {
            segmented_changelog_tailer
                .once(&ctx)
                .await
                .with_context(|| format!("repo {}: incrementally building repo", repo_id))?;
            info!(
                ctx.logger(),
                "repo {}: SegmentedChangelogTailer is done", repo_id,
            );
        } else {
            let delay = Duration::from_secs(args::get_u64(matches, DELAY_ARG, 300));
            // spread out repo operations
            let offset_delay = delay / repo_count;
            let ctx = ctx.clone();
            tasks.push(async move {
                tokio::time::delay_for(offset_delay * index as u32).await;
                segmented_changelog_tailer.run(&ctx, delay).await;
            });
        }
    }

    join_all(tasks).await;


    Ok(())
}
