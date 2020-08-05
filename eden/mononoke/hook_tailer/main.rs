/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

pub mod tailer;

use anyhow::{format_err, Error, Result};
use blobrepo::BlobRepo;
use blobrepo_factory::BlobrepoBuilder;
use bookmarks::BookmarkName;
use clap::{App, Arg, ArgMatches};
use cmdlib::helpers::{block_execute, csid_resolve};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    future,
    stream::{FuturesUnordered, StreamExt, TryStreamExt},
};
use mononoke_types::ChangesetId;
use slog::{debug, info, Logger};
use std::collections::HashSet;
use std::time::Duration;
use time_ext::DurationExt;
use tokio::{
    fs::{File, OpenOptions},
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
};

use tailer::{HookExecutionInstance, Tailer};

async fn get_changesets<'a>(
    matches: &'a ArgMatches<'a>,
    inline_arg: &str,
    file_arg: &str,
    ctx: &CoreContext,
    repo: &BlobRepo,
) -> Result<HashSet<ChangesetId>> {
    let mut ids = matches
        .values_of(inline_arg)
        .map(|matches| matches.map(|cs| cs.to_string()).collect())
        .unwrap_or_else(|| vec![]);

    if let Some(path) = matches.value_of(file_arg) {
        let file = File::open(path).await?;
        let mut lines = BufReader::new(file).lines();
        while let Some(line) = lines.next().await {
            ids.push(line?);
        }
    }

    let ret = ids
        .into_iter()
        .map(|cs| csid_resolve(ctx.clone(), repo.clone(), cs).compat())
        .collect::<FuturesUnordered<_>>()
        .try_collect()
        .await?;

    Ok(ret)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let matches = setup_app().get_matches();
    let (repo_name, config) = cmdlib::args::get_config(fb, &matches)?;
    let logger = cmdlib::args::init_logging(fb, &matches);
    info!(logger, "Hook tailer is starting");

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    block_execute(
        run_hook_tailer(fb, &ctx, &config, &repo_name, &matches, &logger),
        fb,
        "hook_tailer",
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}

async fn run_hook_tailer<'a>(
    fb: FacebookInit,
    ctx: &CoreContext,
    config: &metaconfig_types::RepoConfig,
    repo_name: &str,
    matches: &'a ArgMatches<'a>,
    logger: &Logger,
) -> Result<(), Error> {
    let bookmark_name = matches.value_of("bookmark").unwrap();
    let bookmark = BookmarkName::new(bookmark_name)?;
    let common_config = cmdlib::args::load_common_config(fb, &matches)?;
    let limit = cmdlib::args::get_usize(&matches, "limit", 1000);
    let concurrency = cmdlib::args::get_usize(&matches, "concurrency", 100);
    let log_interval = cmdlib::args::get_usize(&matches, "log_interval", 500);
    let exclude_merges = matches.is_present("exclude_merges");
    let stats_file = matches.value_of("stats_file");

    let mut stats_file = match stats_file {
        Some(stats_file) => {
            let mut stats_file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(stats_file)
                .await?;

            let header = "Changeset ID,File Count,Outcomes,Completion Time us,Poll Time us\n";
            stats_file.write_all(header.as_ref()).await?;

            Some(stats_file)
        }
        None => None,
    };

    let disabled_hooks = cmdlib::args::parse_disabled_hooks_no_repo_prefix(&matches, &logger);

    let caching = cmdlib::args::init_cachelib(fb, &matches, None);
    let readonly_storage = cmdlib::args::parse_readonly_storage(&matches);
    let builder = BlobrepoBuilder::new(
        fb,
        repo_name.into(),
        &config,
        cmdlib::args::parse_mysql_options(&matches),
        caching,
        common_config.scuba_censored_table,
        readonly_storage,
        cmdlib::args::parse_blobstore_options(&matches),
        &logger,
    );

    let blobrepo = builder.build().await?;

    let (exclusions, inclusions) = future::try_join(
        get_changesets(matches, "exclude", "exclude_file", &ctx, &blobrepo),
        get_changesets(matches, "changeset", "changeset_file", &ctx, &blobrepo),
    )
    .await?;

    let tail = &Tailer::new(
        ctx.clone(),
        blobrepo.clone(),
        config.clone(),
        bookmark,
        concurrency,
        log_interval,
        exclude_merges,
        exclusions,
        &disabled_hooks,
    )
    .await?;

    let mut stream = if inclusions.is_empty() {
        tail.run_with_limit(limit).boxed()
    } else {
        tail.run_changesets(inclusions).boxed()
    };

    let mut summary = HookExecutionSummary::default();

    info!(logger, "==== Hooks results ====");

    while let Some(instance) = stream.next().await {
        let instance = instance?;

        if let Some(ref mut stats_file) = stats_file {
            let line = format!(
                "{},{},{},{},{}\n",
                instance.cs_id,
                instance.file_count,
                instance.outcomes.len(),
                instance.stats.completion_time.as_micros_unchecked(),
                instance.stats.poll_time.as_micros_unchecked(),
            );
            stats_file.write_all(line.as_ref()).await?;
        }

        summary.add_instance(&instance, &logger);
    }

    info!(logger, "==== Hooks stats ====");
    info!(
        logger,
        "Completion time: {}us",
        summary.completion_time.as_micros_unchecked()
    );
    info!(
        logger,
        "Poll time: {}us",
        summary.poll_time.as_micros_unchecked()
    );
    info!(logger, "Changesets accepted: {}", summary.accepted);
    info!(logger, "Changesets rejected: {}", summary.rejected);

    if summary.rejected > 0 {
        return Err(format_err!("Hook rejections: {}", summary.rejected));
    }

    Ok(())
}

#[derive(Default)]
struct HookExecutionSummary {
    accepted: u64,
    rejected: u64,
    completion_time: Duration,
    poll_time: Duration,
}

impl HookExecutionSummary {
    pub fn add_instance(&mut self, instance: &HookExecutionInstance, logger: &Logger) {
        let mut is_rejected = false;

        for outcome in instance.outcomes.iter() {
            if outcome.is_rejection() {
                is_rejected = true;
                info!(logger, "{}", outcome);
            } else {
                debug!(logger, "{}", outcome);
            }
        }

        if is_rejected {
            self.rejected += 1;
        } else {
            self.accepted += 1;
        }

        self.completion_time += instance.stats.completion_time;
        self.poll_time += instance.stats.poll_time;
    }
}

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    let app = cmdlib::args::MononokeApp::new("run hooks against repo")
        .with_advanced_args_hidden()
        .build()
        .version("0.0.0")
        .arg(
            Arg::with_name("bookmark")
                .long("bookmark")
                .short("B")
                .help("bookmark to tail")
                .takes_value(true)
                .required(true),
        )
        .arg(
            Arg::with_name("concurrency")
                .long("concurrency")
                .help("the number of changesets to run hooks for in parallel")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("log_interval")
                .long("log-interval")
                .help("show progress by logging every N commits")
                .takes_value(true)
                .default_value("500"),
        )
        .arg(
            Arg::with_name("changeset")
                .long("changeset")
                .short("c")
                .multiple(true)
                .help("the changeset to run hooks for")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("changeset_file")
                .long("changeset-file")
                .alias("changeset_file")
                .help("a file containing chnagesets to explicitly run hooks for")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("exclude")
                .long("exclude")
                .short("e")
                .multiple(true)
                .help("the changesets to exclude")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("exclude_file")
                .long("exclude-file")
                .alias("exclude_file")
                .short("f")
                .help("a file containing changesets to exclude that is separated by new lines")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("exclude_merges")
                .long("exclude-merges")
                .help("exclude changesets that are merges (more than one parent)"),
        )
        .arg(
            Arg::with_name("limit")
                .long("limit")
                .takes_value(true)
                .help("limit number of commits to process (non-continuous only). Default: 1000"),
        )
        .arg(
            Arg::with_name("stats_file")
                .long("stats-file")
                .takes_value(true)
                .help("Log hook execution statistics to a file (CSV format)"),
        );

    cmdlib::args::add_disabled_hooks_args(app)
}
