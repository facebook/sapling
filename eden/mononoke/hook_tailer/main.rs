/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

pub mod tailer;

use anyhow::{format_err, Error, Result};
use blobrepo_factory::BlobrepoBuilder;
use bookmarks::BookmarkName;
use clap::{App, Arg, ArgMatches};
use cmdlib::helpers::{block_execute, csid_resolve};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    stream::{self, StreamExt},
};
use mercurial_types::HgChangesetId;
use slog::{debug, info, Logger};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::iter::Extend;
use std::str::FromStr;
use std::time::Duration;
use thiserror::Error;
use time_ext::DurationExt;
use tokio::{fs::OpenOptions, io::AsyncWriteExt};

use tailer::{HookExecutionInstance, Tailer};

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
    let common_config = cmdlib::args::read_common_config(fb, &matches)?;
    let limit = cmdlib::args::get_usize(&matches, "limit", 1000);
    let changeset = matches.value_of("changeset");
    let stats_file = matches.value_of("stats-file");

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

    let mut excludes = matches
        .values_of("exclude")
        .map(|matches| {
            matches
                .map(|cs| HgChangesetId::from_str(cs).expect("Invalid changeset"))
                .collect()
        })
        .unwrap_or(vec![]);

    if let Some(path) = matches.value_of("exclude_file") {
        let changesets = BufReader::new(File::open(path)?)
            .lines()
            .filter_map(|cs_str| {
                cs_str
                    .map_err(Error::from)
                    .and_then(|cs_str| HgChangesetId::from_str(&cs_str))
                    .ok()
            });

        excludes.extend(changesets);
    }

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

    let changeset = match changeset {
        Some(changeset) => Some(
            csid_resolve(ctx.clone(), blobrepo.clone(), changeset)
                .compat()
                .await?,
        ),
        None => None,
    };

    let excl = blobrepo
        .get_hg_bonsai_mapping(ctx.clone(), excludes)
        .compat()
        .await?;

    let tail = &Tailer::new(
        ctx.clone(),
        blobrepo.clone(),
        config.clone(),
        bookmark,
        excl.into_iter().map(|(_, cs)| cs).collect(),
        &disabled_hooks,
    )?;

    let mut stream = match changeset {
        Some(changeset) => stream::once(tail.run_single_changeset(changeset)).boxed(),
        None => tail.run_with_limit(limit).boxed(),
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
            Arg::with_name("changeset")
                .long("changeset")
                .short("c")
                .help("the changeset to run hooks for")
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
                .long("exclude_file")
                .short("f")
                .help("a file containing changesets to exclude that is separated by new lines")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("limit")
                .long("limit")
                .takes_value(true)
                .help("limit number of commits to process (non-continuous only). Default: 1000"),
        )
        .arg(
            Arg::with_name("stats-file")
                .long("stats-file")
                .takes_value(true)
                .help("Log hook execution statistics to a file (CSV format)"),
        );

    cmdlib::args::add_disabled_hooks_args(app)
}

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("No such repo '{0}'")]
    NoSuchRepo(String),
}
