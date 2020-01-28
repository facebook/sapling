/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

mod dispatcher;
mod protocol;

use anyhow::Error;
use clap::{Arg, ArgMatches};
use cloned::cloned;
use cmdlib::args;
use context::SessionContainer;
use fbinit::FacebookInit;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{self, FutureExt},
    stream::TryStreamExt,
};
use futures_stats::TimedFutureExt;
use hgproto::HgCommands;
use hooks::HookManager;
use hooks_content_stores::{InMemoryChangesetStore, InMemoryFileContentStore};
use metaconfig_types::HookManagerParams;
use repo_client::MononokeRepoBuilder;
use scuba_ext::ScubaSampleBuilder;
use scuba_ext::ScubaSampleBuilderExt;
use slog::{info, o, warn, Logger};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::{
    io::{self, AsyncBufReadExt, BufReader},
    task::JoinHandle,
};

use dispatcher::FastReplayDispatcher;
use protocol::{RepoRequest, Request};

const ARG_NO_SKIPLIST: &str = "no-skiplist";
const ARG_NO_CACHE_WARMUP: &str = "no-cache-warmup";
const ARG_ALIASES: &str = "alias";
const ARG_MAX_CONCURRENCY: &str = "max-concurrency";

fn dispatch(
    scuba: &ScubaSampleBuilder,
    dispatchers: &HashMap<String, FastReplayDispatcher>,
    aliases: &HashMap<String, String>,
    req: &str,
    count: &Arc<AtomicU64>,
) -> Result<JoinHandle<()>, Error> {
    let req: RepoRequest = req.parse()?;

    let reponame = aliases
        .get(&req.reponame)
        .map(|r| r.to_string())
        .unwrap_or(req.reponame);

    let dispatcher = dispatchers
        .get(&reponame)
        .ok_or_else(|| Error::msg(format!("Repository does not exist: {}", reponame)))?;

    let stream = match req.request {
        Request::Gettreepack(args) => dispatcher.client().gettreepack(args).compat(),
    };

    count.fetch_add(1, Ordering::Relaxed);

    let task = {
        cloned!(count, mut scuba);

        scuba.add("reponame", reponame);

        async move {
            let (stats, res) = stream
                .try_fold(0, |size, e| future::ready(Ok(size + e.len())))
                .timed()
                .await;
            scuba.add_future_stats(&stats);

            match res {
                Ok(size) => {
                    scuba.add("replay_response_size", size);
                    scuba.log_with_msg("Replay Succeeded", None);
                }
                Err(e) => {
                    scuba.log_with_msg("Replay Failed", format!("{:#?}", e));
                }
            }

            count.fetch_sub(1, Ordering::Relaxed);

            ()
        }
    };

    Ok(tokio::task::spawn(task.boxed()))
}

fn build_noop_hook_manager(fb: FacebookInit) -> HookManager {
    HookManager::new(
        fb,
        Box::new(InMemoryChangesetStore::new()),
        Arc::new(InMemoryFileContentStore::new()),
        HookManagerParams {
            disable_acl_checker: true,
        },
        ScubaSampleBuilder::with_discard(),
    )
}

fn extract_alias(alias: &str) -> Result<(String, String), Error> {
    let mut iter = alias.split(":");

    match (iter.next(), iter.next(), iter.next()) {
        (Some(from), Some(to), None) => Ok((from.to_string(), to.to_string())),
        _ => {
            let e = Error::msg(format!("Invalid alias: {} (expected FROM:TO)", alias));
            Err(e)
        }
    }
}

async fn bootstrap_repositories<'a>(
    fb: FacebookInit,
    matches: &ArgMatches<'a>,
    logger: &Logger,
    scuba: &ScubaSampleBuilder,
) -> Result<HashMap<String, FastReplayDispatcher>, Error> {
    let config = args::read_configs(fb, &matches)?;

    let mysql_options = cmdlib::args::parse_mysql_options(&matches);
    let caching = cmdlib::args::init_cachelib(fb, &matches);
    let readonly_storage = cmdlib::args::parse_readonly_storage(&matches);
    let blobstore_options = cmdlib::args::parse_blobstore_options(&matches);

    let no_skiplist = matches.is_present(ARG_NO_SKIPLIST);
    let no_cache_warmup = matches.is_present(ARG_NO_CACHE_WARMUP);

    let noop_hook_manager = Arc::new(build_noop_hook_manager(fb));

    info!(&logger, "Creating {} repositories", config.repos.len());

    let repos = future::try_join_all(config.repos.into_iter().map(|(name, mut config)| {
        let noop_hook_manager = &noop_hook_manager;
        let blobstore_options = &blobstore_options;

        let logger = logger.new(o!("repo" => name.clone()));

        let bootstrap_ctx = {
            let session = SessionContainer::new_with_defaults(fb);
            session.new_context(logger.clone(), scuba.clone())
        };

        if no_skiplist {
            config.skiplist_index_blobstore_key = None;
        }

        async move {
            let warmup_params = config.cache_warmup.clone();

            let repo = MononokeRepoBuilder::prepare(
                bootstrap_ctx.clone(),
                name.clone(),
                config,
                mysql_options,
                caching,
                None, // Don't report censored blob access
                readonly_storage,
                blobstore_options.clone(),
            )
            .await?
            .finalize(noop_hook_manager.clone())
            .await?;

            let warmup = if no_cache_warmup {
                None
            } else {
                let handle = tokio::task::spawn(
                    cache_warmup::cache_warmup(
                        bootstrap_ctx.clone(),
                        repo.blobrepo().clone(),
                        warmup_params,
                    )
                    .compat(),
                );
                Some(handle)
            };

            let dispatcher = FastReplayDispatcher::new(fb, logger.clone(), scuba.clone(), repo);

            if let Some(warmup) = warmup {
                info!(&logger, "Waiting for cache warmup to complete...");
                warmup.await??;
            }

            Result::<_, Error>::Ok((name, dispatcher))
        }
    }))
    .await?
    .into_iter()
    .collect();

    info!(&logger, "Repositories are ready!");

    Ok(repos)
}

struct ReplayOpts {
    max_concurrency: u64,
    aliases: HashMap<String, String>,
}

impl ReplayOpts {
    fn parse<'a>(matches: &ArgMatches<'a>) -> Result<Self, Error> {
        let max_concurrency = matches
            .value_of(ARG_MAX_CONCURRENCY)
            .map(|n| -> Result<u64, Error> {
                let n = n.parse()?;
                Ok(n)
            })
            .transpose()?
            .unwrap_or(50);

        let aliases = match matches.values_of(ARG_ALIASES) {
            Some(values) => values
                .into_iter()
                .map(extract_alias)
                .collect::<Result<HashMap<_, _>, _>>()?,
            None => HashMap::new(),
        };

        Ok(Self {
            max_concurrency,
            aliases,
        })
    }
}

async fn fast_replay_from_stdin<'a>(
    opts: &ReplayOpts,
    logger: &Logger,
    scuba: &ScubaSampleBuilder,
    repos: &HashMap<String, FastReplayDispatcher>,
    count: &Arc<AtomicU64>,
) -> Result<(), Error> {
    let mut reader = BufReader::new(io::stdin()).lines();

    loop {
        let load = count.load(Ordering::Relaxed);
        if load > opts.max_concurrency {
            warn!(
                &logger,
                "Waiting for some requests to complete (load: {})...", load
            );
            tokio::time::delay_for(Duration::from_millis(100)).await;
            continue;
        }

        match reader.next_line().await? {
            Some(req) => match dispatch(&scuba, &repos, &opts.aliases, &req, &count) {
                Ok(..) => {
                    continue;
                }
                Err(e) => {
                    warn!(&logger, "Dispatch failed: {}", e);
                    continue;
                }
            },
            None => {
                info!(&logger, "Processed all input...");
                return Ok(());
            }
        }
    }
}

async fn do_main<'a>(fb: FacebookInit, matches: &ArgMatches<'a>) -> Result<(), Error> {
    let logger = args::init_logging(fb, &matches);

    let mut scuba = args::get_scuba_sample_builder(fb, &matches)?;
    scuba.add_common_server_data();

    // Do this earlier to show errors before we bootstrap repositories
    let opts = ReplayOpts::parse(&matches)?;

    let repos = bootstrap_repositories(fb, &matches, &logger, &scuba).await?;

    let count = Arc::new(AtomicU64::new(0));
    fast_replay_from_stdin(&opts, &logger, &scuba, &repos, &count).await?;

    loop {
        let n = count.load(Ordering::Relaxed);
        if n == 0 {
            break;
        }
        info!(&logger, "Waiting for {} requests to finish...", n);
        tokio::time::delay_for(Duration::from_millis(1000)).await;
    }

    Ok(())
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeApp::new("Mononoke Local Replay")
        .with_advanced_args_hidden()
        .with_all_repos()
        .with_scuba_logging_args()
        .build()
        .arg(
            Arg::with_name(ARG_MAX_CONCURRENCY)
                .long(ARG_MAX_CONCURRENCY)
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_NO_SKIPLIST)
                .long(ARG_NO_SKIPLIST)
                .takes_value(false)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_NO_CACHE_WARMUP)
                .long(ARG_NO_CACHE_WARMUP)
                .takes_value(false)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_ALIASES)
                .long(ARG_ALIASES)
                .help("Map a repository name to replay to one found in config (FROM:TO)")
                .multiple(true)
                .number_of_values(1)
                .takes_value(true)
                .required(false),
        );

    let matches = app.get_matches();

    let mut runtime = args::init_runtime(&matches)?;
    runtime.block_on_std(do_main(fb, &matches))?;

    Ok(())
}
