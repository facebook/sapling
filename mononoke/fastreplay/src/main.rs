/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

mod config;
mod dispatcher;
mod protocol;

use anyhow::{Context, Error};
use blobstore_factory::make_blobstore_no_sql;
use clap::{Arg, ArgMatches};
use cloned::cloned;
use cmdlib::{args, monitoring::ReadyFlagService};
use configerator_cached::ConfigHandle;
use context::SessionContainer;
use fbinit::FacebookInit;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{self, FutureExt},
    stream::{self, StreamExt, TryStreamExt},
};
use futures_stats::TimedFutureExt;
use hgproto::HgCommands;
use hooks::HookManager;
use hooks_content_stores::{InMemoryChangesetStore, InMemoryFileContentStore};
use metaconfig_types::HookManagerParams;
use mononoke_types::Timestamp;
use rand::{thread_rng, Rng};
use repo_client::MononokeRepoBuilder;
use scopeguard::defer;
use scuba_ext::ScubaSampleBuilder;
use scuba_ext::ScubaSampleBuilderExt;
use slog::{debug, info, o, warn, Logger};
use stats::prelude::*;
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::{
    io::{self, AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
};

use config::FastReplayConfig;
use dispatcher::FastReplayDispatcher;
use protocol::{Request, RequestLine};

define_stats! {
    prefix = "mononoke.fastreplay";
    received: timeseries(Rate, Sum),
    admitted: timeseries(Rate, Sum),
    processed: timeseries(Rate, Sum),
}

const ARG_NO_SKIPLIST: &str = "no-skiplist";
const ARG_NO_CACHE_WARMUP: &str = "no-cache-warmup";
const ARG_ALIASES: &str = "alias";
const ARG_HASH_VALIDATION_PERCENTAGE: &str = "hash-validation-percentage";
const ARG_LIVE_CONFIG: &str = "live-config";
const ARG_COMMAND: &str = "command";

const LIVE_CONFIG_POLL_INTERVAL: u64 = 5;

fn should_admit(config: &FastReplayConfig) -> bool {
    let admission_rate = config.admission_rate();

    if admission_rate >= 100 {
        return true;
    }

    if admission_rate <= 0 {
        return false;
    }

    let roll = thread_rng().gen_range(1, 100);
    roll <= admission_rate
}

async fn dispatch(
    dispatchers: &HashMap<String, FastReplayDispatcher>,
    aliases: &HashMap<String, String>,
    req: &str,
    mut scuba: ScubaSampleBuilder,
) -> Result<(), Error> {
    let req = serde_json::from_str::<RequestLine>(&req)?;

    let reponame = aliases
        .get(req.normal.reponame.as_ref())
        .map(|a| a.as_ref())
        .unwrap_or(req.normal.reponame.as_ref())
        .to_string();

    let dispatcher = dispatchers
        .get(&reponame)
        .ok_or_else(|| Error::msg(format!("Repository does not exist: {}", reponame)))?;

    let parsed_req = protocol::parse_request(&req, &dispatcher)
        .await
        .context("While parsing request")?;

    scuba.add("reponame", reponame);
    let client = dispatcher.client(scuba.clone());

    let stream = match parsed_req {
        Request::Gettreepack(args) => client.gettreepack(args.0).compat(),
        Request::Getbundle(args) => client.getbundle(args.0).compat(),
        Request::GetpackV1(args) => client
            .getpackv1(Box::new(
                stream::iter(args.entries.into_iter().map(Ok).collect::<Vec<_>>())
                    .boxed()
                    .compat(),
            ))
            .compat(),
        Request::GetpackV2(args) => client
            .getpackv2(Box::new(
                stream::iter(args.entries.into_iter().map(Ok).collect::<Vec<_>>())
                    .boxed()
                    .compat(),
            ))
            .compat(),
    };

    scuba.add("command", req.normal.command.as_ref());
    if let Some(args) = req.normal.args.as_ref() {
        scuba.add("command_args", args.as_ref());
    }
    if let Some(remote_args) = req.normal.remote_args.as_ref() {
        scuba.add("command_remote_args", remote_args.as_ref());
    }

    let replay_delay = Timestamp::from_timestamp_secs(req.int.time).since_seconds();
    scuba.add("replay_delay_s", replay_delay);

    scuba.add("recorded_server", req.server_type());
    scuba.add("recorded_duration_us", req.duration_us());

    if let Some(responselen) = req.int.responselen {
        scuba.add("recorded_response_length", responselen);
    }
    if let Some(session_id) = req.normal.mononoke_session_uuid {
        scuba.add("recorded_mononoke_session_id", session_id.as_ref());
    }

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

    Ok(())
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
    let caching = cmdlib::args::init_cachelib(fb, &matches, None);
    let readonly_storage = cmdlib::args::parse_readonly_storage(&matches);
    let blobstore_options = cmdlib::args::parse_blobstore_options(&matches);

    let no_skiplist = matches.is_present(ARG_NO_SKIPLIST);
    let no_cache_warmup = matches.is_present(ARG_NO_CACHE_WARMUP);
    let hash_validation_percentage = matches
        .value_of(ARG_HASH_VALIDATION_PERCENTAGE)
        .map(|n| -> Result<usize, Error> {
            let n = n.parse()?;
            Ok(n)
        })
        .transpose()?
        .unwrap_or(0);

    let noop_hook_manager = Arc::new(build_noop_hook_manager(fb));

    info!(&logger, "Creating {} repositories", config.repos.len());

    let repos = future::try_join_all(config.repos.into_iter().map(|(name, mut config)| {
        let noop_hook_manager = &noop_hook_manager;
        let blobstore_options = &blobstore_options;

        let logger = logger.new(o!("repo" => name.clone()));

        let bootstrap_ctx = {
            let mut scuba = scuba.clone();
            scuba.add("reponame", name.clone());
            let session = SessionContainer::new_with_defaults(fb);
            session.new_context(logger.clone(), scuba.clone())
        };

        if no_skiplist {
            config.skiplist_index_blobstore_key = None;
        }

        async move {
            let warmup_params = config.cache_warmup.clone();

            // If we have remote args support for this repo, let's open it now. Note that
            // this requires using prod configs for Fastreplay since those are the ones with
            // wireproto logging config.
            let remote_args_blobstore = config
                .wireproto_logging
                .storage_config_and_threshold
                .as_ref()
                .map(|(storage, _)| {
                    make_blobstore_no_sql(fb, &storage.blobstore, readonly_storage).compat()
                });

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

            let remote_args_blobstore = match remote_args_blobstore {
                Some(fut) => Some(fut.await?),
                None => None,
            };

            let dispatcher = FastReplayDispatcher::new(
                fb,
                logger.clone(),
                repo,
                remote_args_blobstore,
                hash_validation_percentage,
            )?;

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
    aliases: Arc<HashMap<String, String>>,
    config: ConfigHandle<FastReplayConfig>,
}

impl ReplayOpts {
    fn parse<'a>(
        fb: FacebookInit,
        logger: Logger,
        matches: &ArgMatches<'a>,
    ) -> Result<Self, Error> {
        let aliases = match matches.values_of(ARG_ALIASES) {
            Some(values) => values
                .into_iter()
                .map(extract_alias)
                .collect::<Result<HashMap<_, _>, _>>()?,
            None => HashMap::new(),
        };
        let aliases = Arc::new(aliases);

        let config = cmdlib::args::get_config_handle(
            fb,
            logger,
            matches.value_of(ARG_LIVE_CONFIG),
            LIVE_CONFIG_POLL_INTERVAL,
        )
        .with_context(|| format!("While parsing --{}", ARG_LIVE_CONFIG))?;

        Ok(Self { aliases, config })
    }
}

async fn fastreplay<R: AsyncRead + Unpin>(
    opts: &ReplayOpts,
    reader: R,
    logger: &Logger,
    scuba: &ScubaSampleBuilder,
    repos: &Arc<HashMap<String, FastReplayDispatcher>>,
    count: &Arc<AtomicU64>,
) -> Result<(), Error> {
    let mut reader = BufReader::new(reader).lines();

    loop {
        let load = count.load(Ordering::Relaxed);

        let config = opts.config.get();

        if load > config.max_concurrency()?.get() {
            warn!(
                &logger,
                "Waiting for some requests to complete (load: {})...", load
            );
            tokio::time::delay_for(Duration::from_millis(100)).await;
            continue;
        }

        let line = reader.next_line().await?;
        STATS::received.add_value(1);

        match line {
            Some(req) => {
                if !should_admit(&config) {
                    debug!(&logger, "Request was not admitted");
                    continue;
                }
                STATS::admitted.add_value(1);

                count.fetch_add(1, Ordering::Relaxed);

                // NOTE: We clone values here because we need a 'static future to spawn.
                cloned!(logger, mut scuba, repos, opts.aliases, count);

                scuba.sampled(config.scuba_sampling_target()?);

                let task = async move {
                    defer!({
                        count.fetch_sub(1, Ordering::Relaxed);
                    });

                    match dispatch(&repos, &aliases, &req, scuba).await {
                        Ok(()) => {
                            STATS::processed.add_value(1);
                        }
                        Err(err) => {
                            warn!(&logger, "Dispatch failed: {:#?}", err);
                        }
                    };
                };

                tokio::task::spawn(task.boxed());
            }
            None => {
                info!(&logger, "Processed all input...");
                return Ok(());
            }
        };
    }
}

async fn do_main<'a>(
    fb: FacebookInit,
    matches: &ArgMatches<'a>,
    logger: &Logger,
    service: &ReadyFlagService,
) -> Result<(), Error> {
    let mut scuba = args::get_scuba_sample_builder(fb, &matches)?;
    scuba.add_common_server_data();

    // Do this earlier to show errors before we bootstrap repositories
    let opts = ReplayOpts::parse(fb, logger.clone(), &matches)?;

    let repos = bootstrap_repositories(fb, &matches, &logger, &scuba).await?;
    let repos = Arc::new(repos);

    // Report that we're good to go.
    service.set_ready();

    // Start replaying
    let count = Arc::new(AtomicU64::new(0));

    match matches.values_of_os(ARG_COMMAND) {
        Some(mut args) => {
            let program = args
                .next()
                .ok_or_else(|| Error::msg("Command cannot be empty"))?;

            let mut command = Command::new(program);

            command
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit());

            for arg in args {
                command.arg(arg);
            }

            let mut child = command.spawn()?;
            let stdout = child.stdout.take().expect("Stdout was piped()");
            fastreplay(&opts, stdout, &logger, &scuba, &repos, &count).await?;

            // Wait for child to terminate
            child.await?;
        }
        None => {
            fastreplay(&opts, io::stdin(), &logger, &scuba, &repos, &count).await?;
        }
    }

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
        .with_fb303_args()
        .with_all_repos()
        .with_scuba_logging_args()
        .build()
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
        )
        .arg(
            Arg::with_name(ARG_HASH_VALIDATION_PERCENTAGE)
                .long(ARG_HASH_VALIDATION_PERCENTAGE)
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_LIVE_CONFIG)
                .long(ARG_LIVE_CONFIG)
                .help("Path to read hot-reloadable configuration from")
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_COMMAND)
                .help("Command to run to fetch traffic to replay (defaults to reading from stdin otherwise)")
                .takes_value(true)
                .multiple(true)
                .required(false)
        );

    let matches = app.get_matches();

    let logger = args::init_logging(fb, &matches);
    let service = ReadyFlagService::new();

    let main = do_main(fb, &matches, &logger, &service);

    cmdlib::helpers::block_execute(main, fb, "fastreplay", &logger, &matches, service.clone())?;

    Ok(())
}
