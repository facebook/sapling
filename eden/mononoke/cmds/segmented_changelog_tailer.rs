/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::time::Duration;

use anyhow::{format_err, Context, Error};
use blobrepo::BlobRepo;
use bytes::Bytes;
use clap::Arg;
use futures::future::join_all;
use futures::stream;
use slog::{error, info};

use changesets::deserialize_cs_entries;
use cmdlib::{
    args::{self, MononokeMatches},
    helpers,
};
use context::{CoreContext, SessionContainer};
use fbinit::FacebookInit;
use segmented_changelog::{seedheads_from_config, SegmentedChangelogTailer};

const ONCE_ARG: &str = "once";
const REPO_ARG: &str = "repo";
const ARG_PREFETCHED_COMMITS_PATH: &str = "prefetched-commits-path";

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
        )
        .arg(
            Arg::with_name(ARG_PREFETCHED_COMMITS_PATH)
                .long(ARG_PREFETCHED_COMMITS_PATH)
                .takes_value(true)
                .required(false)
                .help(
                    "a file with a serialized list of ChangesetEntry, \
                which can be used to speed up rebuilding of segmented changelog",
                ),
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

    let prefetched_commits = match matches.value_of(ARG_PREFETCHED_COMMITS_PATH) {
        Some(path) => {
            info!(ctx.logger(), "reading prefetched commits from {}", path);
            let data = tokio::fs::read(path).await?;
            deserialize_cs_entries(&Bytes::from(data))
                .with_context(|| format!("failed to parse serialized cs entries from {}", path))?
        }
        None => vec![],
    };

    let config_store = matches.config_store();
    let mysql_options = matches.mysql_options();
    let configs = args::load_repo_configs(config_store, matches)?;

    let mut tasks = Vec::new();
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

        let seed_heads = seedheads_from_config(&ctx, &config.segmented_changelog_config)?;

        // This is a bit weird from the dependency point of view but I think that it is best. The
        // BlobRepo may have a SegmentedChangelog attached to it but that doesn't hurt us in any
        // way.  On the other hand reconstructing the dependencies for SegmentedChangelog without
        // BlobRepo is probably prone to more problems from the maintenance perspective.
        let blobrepo: BlobRepo =
            args::open_repo_with_repo_id(ctx.fb, ctx.logger(), repo_id, matches).await?;

        let prefetched_commits = stream::iter(prefetched_commits.iter().filter_map(|entry| {
            if entry.repo_id == repo_id {
                Some(Ok(entry.clone()))
            } else {
                None
            }
        }));

        let segmented_changelog_tailer = SegmentedChangelogTailer::build_from(
            &ctx,
            &blobrepo,
            &config.storage_config.metadata,
            mysql_options,
            seed_heads,
            prefetched_commits,
            None,
        )
        .await?;

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
