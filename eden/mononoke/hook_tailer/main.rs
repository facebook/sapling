/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![feature(never_type)]

pub mod tailer;

use anyhow::{format_err, Error, Result};
use blobrepo_factory::BlobrepoBuilder;
use bookmarks::BookmarkName;
use clap::{App, Arg, ArgMatches};
use cmdlib::helpers::{block_execute, csid_resolve};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::{compat::Future01CompatExt, future::Future};
use futures_stats::TimedFutureExt;
use hooks::HookOutcome;
use mercurial_types::HgChangesetId;
use slog::{debug, info, Logger};
use std::fmt;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::str::FromStr;
use tailer::Tailer;
use thiserror::Error;
use time_ext::DurationExt;

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

    match changeset {
        Some(changeset) => process_hook_results(tail.run_single_changeset(changeset), logger).await,
        None => process_hook_results(tail.run_with_limit(limit), logger).await,
    }
}

async fn process_hook_results<F: Future<Output = Result<Vec<HookOutcome>, Error>>>(
    fut: F,
    logger: &Logger,
) -> Result<(), Error> {
    let (stats, res) = fut.timed().await;
    let res = res?;

    let mut hooks_stat = HookExecutionStat::new();

    debug!(logger, "==== Hooks results ====");
    res.into_iter().for_each(|outcome| {
        hooks_stat.record_hook_execution(&outcome);

        if outcome.is_rejection() {
            info!(logger, "{}", outcome);
        } else {
            debug!(logger, "{}", outcome);
        }
    });

    info!(logger, "==== Hooks stat: {} ====", hooks_stat);
    info!(
        logger,
        "==== Completion Time: {}us, Poll time: {}us ===",
        stats.completion_time.as_micros_unchecked(),
        stats.poll_time.as_micros_unchecked(),
    );

    if hooks_stat.rejected > 0 {
        Err(format_err!("Hook rejections: {}", hooks_stat.rejected,))
    } else {
        Ok(())
    }
}

struct HookExecutionStat {
    accepted: usize,
    rejected: usize,
}

impl HookExecutionStat {
    pub fn new() -> Self {
        Self {
            accepted: 0,
            rejected: 0,
        }
    }

    pub fn record_hook_execution(&mut self, outcome: &hooks::HookOutcome) {
        if outcome.is_rejection() {
            self.rejected += 1;
        } else {
            self.accepted += 1;
        }
    }
}

impl fmt::Display for HookExecutionStat {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "accepted: {}, rejected: {}",
            self.accepted, self.rejected
        )
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
        );

    cmdlib::args::add_disabled_hooks_args(app)
}

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("No such repo '{0}'")]
    NoSuchRepo(String),
}
