/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod tailer;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use clap::Arg;
use cmdlib::args::MononokeClapApp;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers::block_execute;
use cmdlib::helpers::csid_resolve;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use hooks::CrossRepoPushSource;
use hooks::PushAuthoredBy;
use mononoke_types::ChangesetId;
use repo_factory::RepoFactory;
use slog::debug;
use slog::info;
use slog::Logger;
use std::collections::HashSet;
use std::time::Duration;
use time_ext::DurationExt;
use tokio::fs::File;
use tokio::fs::OpenOptions;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;

use tailer::HookExecutionInstance;
use tailer::Tailer;

async fn get_changesets<'a>(
    matches: &'a MononokeMatches<'a>,
    inline_arg: &str,
    file_arg: &str,
    ctx: &CoreContext,
    repo: &BlobRepo,
) -> Result<HashSet<ChangesetId>> {
    let mut ids = matches
        .values_of(inline_arg)
        .map_or_else(Vec::new, |matches| {
            matches.map(|cs| cs.to_string()).collect()
        });

    if let Some(path) = matches.value_of(file_arg) {
        let file = File::open(path).await?;
        let mut lines = tokio_stream::wrappers::LinesStream::new(BufReader::new(file).lines());
        while let Some(line) = lines.next().await {
            ids.push(line?);
        }
    }

    let ret = ids
        .into_iter()
        .map(|cs| csid_resolve(ctx, repo, cs))
        .collect::<FuturesUnordered<_>>()
        .try_collect()
        .await?;

    Ok(ret)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let matches = setup_app().get_matches(fb)?;
    let logger = matches.logger();
    let config_store = matches.config_store();
    let (repo_name, config) = cmdlib::args::get_config(config_store, &matches)?;
    info!(logger, "Hook tailer is starting");

    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    block_execute(
        run_hook_tailer(&ctx, config, repo_name, &matches, logger),
        fb,
        "hook_tailer",
        logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}

async fn run_hook_tailer<'a>(
    ctx: &CoreContext,
    config: metaconfig_types::RepoConfig,
    repo_name: String,
    matches: &'a MononokeMatches<'a>,
    logger: &Logger,
) -> Result<(), Error> {
    let config_store = matches.config_store();
    let bookmark_name = matches.value_of("bookmark").unwrap();
    let bookmark = BookmarkName::new(bookmark_name)?;
    let common_config = cmdlib::args::load_common_config(config_store, matches)?;
    let limit = cmdlib::args::get_usize(matches, "limit", 1000);
    let concurrency = cmdlib::args::get_usize(matches, "concurrency", 20);
    let log_interval = cmdlib::args::get_usize(matches, "log_interval", 500);
    let exclude_merges = matches.is_present("exclude_merges");
    let stats_file = matches.value_of("stats_file");
    let cross_repo_push_source = match matches.value_of("push_source") {
        Some("native-to-this-repo") => CrossRepoPushSource::NativeToThisRepo,
        Some("push-redirected") => CrossRepoPushSource::PushRedirected,
        _ => bail!("unexpected value of --push-source"),
    };
    let push_authored_by = match matches.value_of("push_authored_by") {
        Some("user") => PushAuthoredBy::User,
        Some("service") => PushAuthoredBy::Service,
        _ => bail!("unexpected value of --push-authored-by"),
    };

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

    let disabled_hooks = cmdlib::args::parse_disabled_hooks_no_repo_prefix(matches, logger);

    let repo_factory = RepoFactory::new(matches.environment().clone(), &common_config);

    let blobrepo = repo_factory.build(repo_name, config.clone()).await?;

    let (exclusions, inclusions) = future::try_join(
        get_changesets(matches, "exclude", "exclude_file", ctx, &blobrepo),
        get_changesets(matches, "changeset", "changeset_file", ctx, &blobrepo),
    )
    .await?;

    let tail = &Tailer::new(
        ctx.clone(),
        repo_factory.acl_provider(),
        blobrepo.clone(),
        config,
        bookmark,
        concurrency,
        log_interval,
        exclude_merges,
        exclusions,
        &disabled_hooks,
        cross_repo_push_source,
        push_authored_by,
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

        summary.add_instance(&instance, logger);
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

fn setup_app<'a, 'b>() -> MononokeClapApp<'a, 'b> {
    let app = cmdlib::args::MononokeAppBuilder::new("run hooks against repo")
        .with_advanced_args_hidden()
        .with_disabled_hooks_args()
        .build()
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
                .takes_value(true)
                .default_value("20"),
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
                .help("limit number of commits to process (non-continuous only)")
                .default_value("1000"),
        )
        .arg(
            Arg::with_name("stats_file")
                .long("stats-file")
                .takes_value(true)
                .help("Log hook execution statistics to a file (CSV format)"),
        )
        .arg(
            Arg::with_name("push_source")
                .long("push-source")
                .takes_value(true)
                .possible_values(&["native-to-this-repo", "push-redirected"])
                .default_value("native-to-this-repo")
                .help("act as if changesets originated from a given source (see CrossRepoPushSource help for more info)"),
        ).arg(
            Arg::with_name("push_authored_by")
                .long("push-authored-by")
                .takes_value(true)
                .possible_values(&["user", "service"])
                .default_value("user")
                .help("who created changesets (affects how hooks behave, see PushAuthoredBy doc for more info)"),
        );

    app
}
