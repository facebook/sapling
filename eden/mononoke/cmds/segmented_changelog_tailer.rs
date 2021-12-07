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
use blobrepo::BlobRepo;
use clap::Arg;
use futures::future::join_all;
use slog::{error, info};

use blobstore_factory::{make_metadata_sql_factory, ReadOnlyStorage};
use bookmarks::{BookmarkName, Bookmarks};
use cmdlib::{
    args::{self, MononokeMatches},
    helpers,
};
use context::{CoreContext, SessionContainer};
use fbinit::FacebookInit;
use metaconfig_types::MetadataDatabaseConfig;
use segmented_changelog::{SegmentedChangelogSqlConnections, SegmentedChangelogTailer};
use sql_ext::facebook::MyAdmin;
use sql_ext::replication::{NoReplicaLagMonitor, ReplicaLagMonitor};

const ONCE_ARG: &str = "once";
const REPO_ARG: &str = "repo";

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeAppBuilder::new("Updates segmented changelog assets.")
        .with_scuba_logging_args()
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
            Arg::with_name(ONCE_ARG)
                .long(ONCE_ARG)
                .takes_value(false)
                .required(false)
                .help("When set, the tailer will perform a single incremental build run."),
        );
    let matches = app.get_matches(fb)?;

    let logger = matches.logger();
    let session = SessionContainer::new_with_defaults(fb);
    let ctx = session.new_context(logger.clone(), matches.scuba_sample_builder());
    helpers::block_execute(
        run(ctx, &matches),
        fb,
        &std::env::var("TW_JOB_NAME").unwrap_or_else(|_| "segmented_changelog_tailer".to_string()),
        logger,
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

    let config_store = matches.config_store();
    let mysql_options = matches.mysql_options();
    let configs = args::load_repo_configs(config_store, matches)?;
    let readonly_storage = ReadOnlyStorage(false);

    let mut tasks = Vec::new();
    for (index, reponame) in reponames.into_iter().enumerate() {
        let config = configs
            .repos
            .get(&reponame)
            .ok_or_else(|| format_err!("unknown repository: {}", reponame))?;
        let repo_id = config.repoid;

        let bookmarks_name = config
            .segmented_changelog_config
            .master_bookmark
            .as_ref()
            .map(|name| {
                BookmarkName::new(name).with_context(|| {
                    format!(
                        "failed to interpret {} as bookmark for repo {}",
                        name, repo_id
                    )
                })
            })
            .transpose()?;
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
        )
        .await
        .with_context(|| format!("repo {}: constructing metadata sql factory", repo_id))?;

        let segmented_changelog_sql_connections = sql_factory
            .open::<SegmentedChangelogSqlConnections>()
            .with_context(|| {
                format!(
                    "repo {}: error constructing segmented changelog sql connections",
                    repo_id
                )
            })?;

        // This is a bit weird from the dependency point of view but I think that it is best. The
        // BlobRepo may have a SegmentedChangelog attached to it but that doesn't hurt us in any
        // way.  On the other hand reconstructing the dependencies for SegmentedChangelog without
        // BlobRepo is probably prone to more problems from the maintenance perspective.
        let blobrepo: BlobRepo =
            args::open_repo_with_repo_id(ctx.fb, ctx.logger(), repo_id, matches).await?;
        let segmented_changelog_tailer = SegmentedChangelogTailer::new(
            repo_id,
            segmented_changelog_sql_connections,
            replica_lag_monitor,
            blobrepo.get_changeset_fetcher(),
            Arc::new(blobrepo.get_blobstore()),
            Arc::clone(blobrepo.bookmarks()) as Arc<dyn Bookmarks>,
            bookmarks_name,
            None,
        );

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
        } else if let Some(period) = config.segmented_changelog_config.tailer_update_period {
            // spread out update operations, start updates on another repo after 7 seconds
            let wait_to_start = Duration::from_secs(7 * index as u64);
            let ctx = ctx.clone();
            tasks.push(async move {
                tokio::time::sleep(wait_to_start).await;
                segmented_changelog_tailer.run(&ctx, period).await;
            });
        }
    }

    join_all(tasks).await;

    Ok(())
}
