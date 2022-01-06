/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

mod config;
mod dispatcher;
mod protocol;

use anyhow::{Context, Error};
use blobstore_factory::make_blobstore;
use borrowed::borrowed;
use cached_config::ConfigHandle;
use clap::Arg;
use cloned::cloned;
use cmdlib::{
    args::{self, parse_config_spec_to_path, MononokeMatches},
    monitoring::ReadyFlagService,
};
use context::SessionContainer;
use fbinit::FacebookInit;
use futures::{
    compat::Stream01CompatExt,
    future::{self, FutureExt},
    stream::{self, StreamExt, TryStreamExt},
};
use futures_stats::TimedFutureExt;
use hgproto::HgCommands;
use metaconfig_parser::RepoConfigs;
use metaconfig_types::{BlobConfig, CensoredScubaParams};
use mononoke_api::{MononokeApiEnvironment, Repo, WarmBookmarksCacheDerivedData};
use mononoke_types::Timestamp;
use nonzero_ext::nonzero;
use rand::{thread_rng, Rng};
use repo_client::MononokeRepo;
use repo_factory::RepoFactory;
use scopeguard::defer;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::{debug, info, o, warn, Logger};
use stats::prelude::*;
use std::collections::{BTreeSet, HashMap};
use std::num::NonZeroU64;
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
    skipped: timeseries(Rate, Sum),
    replay_success: timeseries(Rate, Sum),
    replay_failure: timeseries(Rate, Sum),
    replay_outcome_permille: timeseries(Average),
}

const ARG_NO_SKIPLIST: &str = "no-skiplist";
const ARG_NO_CACHE_WARMUP: &str = "no-cache-warmup";
const ARG_ALIASES: &str = "alias";
const ARG_HASH_VALIDATION_PERCENTAGE: &str = "hash-validation-percentage";
const ARG_LIVE_CONFIG: &str = "live-config";
const ARG_COMMAND: &str = "command";
const ARG_MULTIPLEXEDBLOB_SAMPLING: &str = "multiplexblob-sampling";

fn should_admit(config: &FastReplayConfig) -> bool {
    let admission_rate = config.admission_rate();

    if admission_rate >= 100 {
        return true;
    }

    if admission_rate <= 0 {
        return false;
    }

    let roll = thread_rng().gen_range(1..100);
    roll <= admission_rate
}

enum DispatchOutcome {
    Processed,
    Skipped,
}

async fn dispatch(
    dispatchers: &HashMap<String, FastReplayDispatcher>,
    aliases: &HashMap<String, String>,
    skipped_repos: &BTreeSet<String>,
    req: &str,
    mut scuba: MononokeScubaSampleBuilder,
) -> Result<DispatchOutcome, Error> {
    let req = serde_json::from_str::<RequestLine>(&req)?;

    let reponame = aliases
        .get(req.normal.reponame.as_ref())
        .map(|a| a.as_ref())
        .unwrap_or(req.normal.reponame.as_ref())
        .to_string();

    if skipped_repos.contains(&reponame) {
        return Ok(DispatchOutcome::Skipped);
    }

    let dispatcher = dispatchers
        .get(&reponame)
        .ok_or_else(|| Error::msg(format!("Repository does not exist: {}", reponame)))?;

    let parsed_req = protocol::parse_request(&req, &dispatcher)
        .await
        .context("While parsing request")?;

    scuba.add("reponame", reponame);
    let client = dispatcher.client(
        scuba.clone(),
        req.normal.client_hostname.clone().map(|s| s.into_owned()),
    );

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
            STATS::replay_success.add_value(1);
            STATS::replay_outcome_permille.add_value(1000);
            scuba.add("replay_response_size", size);
            scuba.log_with_msg("Replay Succeeded", None);
        }
        Err(e) => {
            STATS::replay_failure.add_value(1);
            STATS::replay_outcome_permille.add_value(0);
            scuba
                .unsampled()
                .log_with_msg("Replay Failed", format!("{:#?}", e));
        }
    }

    Ok(DispatchOutcome::Processed)
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
    matches: &MononokeMatches<'a>,
    logger: &Logger,
    scuba: &MononokeScubaSampleBuilder,
) -> Result<HashMap<String, FastReplayDispatcher>, Error> {
    let config_store = matches.config_store();
    let mut config = args::load_repo_configs(config_store, &matches)?;

    // Update the config to something that makes a little more sense for Fastreplay.
    config.common.censored_scuba_params = CensoredScubaParams {
        table: None,
        local_path: None,
    };

    let mysql_options = matches.mysql_options();
    let readonly_storage = matches.readonly_storage();
    let blobstore_options = matches.blobstore_options();

    let repo_factory = RepoFactory::new(matches.environment().clone(), &config.common);

    let no_skiplist = matches.is_present(ARG_NO_SKIPLIST);

    let env = MononokeApiEnvironment {
        repo_factory,
        disabled_hooks: Default::default(),
        warm_bookmarks_cache_derived_data: WarmBookmarksCacheDerivedData::HgOnly,
        warm_bookmarks_cache_enabled: true,
        warm_bookmarks_cache_scuba_sample_builder: MononokeScubaSampleBuilder::with_discard(),
        skiplist_enabled: !no_skiplist,
    };

    let no_cache_warmup = matches.is_present(ARG_NO_CACHE_WARMUP);
    let multiplexblob_sampling_rate = matches
        .value_of(ARG_MULTIPLEXEDBLOB_SAMPLING)
        .map(|n| -> Result<NonZeroU64, Error> {
            let n = n.parse()?;
            Ok(n)
        })
        .transpose()?
        .unwrap_or(nonzero!(1000u64));

    info!(&logger, "Creating {} repositories", config.repos.len());

    let RepoConfigs { repos, common: _ } = config;

    let repos = future::try_join_all(repos.into_iter().map(|(name, mut config)| {
        borrowed!(env, mysql_options, blobstore_options);

        let logger = logger.new(o!("repo" => name.clone()));

        let bootstrap_ctx = {
            let mut scuba = scuba.clone();
            scuba.add("reponame", name.clone());
            let session = SessionContainer::new_with_defaults(fb);
            session.new_context(logger.clone(), scuba.clone())
        };

        // Don't bother starting a hook manager.
        config.hooks = Default::default();
        for book in config.bookmarks.iter_mut() {
            book.hooks = Default::default();
        }

        if no_skiplist {
            config.skiplist_index_blobstore_key = None;
        }

        async move {
            let warmup_params = config.cache_warmup.clone();
            let scrub_handler = &blobstore_factory::default_scrub_handler();

            let remote_args_blobstore = config
                .wireproto_logging
                .storage_config_and_threshold
                .as_ref()
                .map(|(storage, _)| {
                    make_blobstore(
                        fb,
                        storage.blobstore.clone(),
                        mysql_options,
                        *readonly_storage,
                        blobstore_options,
                        &logger,
                        config_store,
                        scrub_handler,
                        None,
                    )
                });

            // Set the Multiplexed blob sampling rate, if used.
            match config.storage_config.blobstore {
                BlobConfig::Multiplexed {
                    ref mut scuba_sample_rate,
                    ..
                } => *scuba_sample_rate = multiplexblob_sampling_rate,
                _ => {}
            };

            let repo_client_knobs = config.repo_client_knobs.clone();

            let repo = Repo::new(&env, name.clone(), config)
                .await
                .context("Error opening Repo")?;
            let repo =
                MononokeRepo::new(fb, Arc::new(repo), &mysql_options, readonly_storage.clone())
                    .await
                    .context("Error opening MononokeRepo")?;

            let warmup = if no_cache_warmup {
                None
            } else {
                let handle = tokio::task::spawn({
                    let blobrepo = repo.blobrepo();
                    cloned!(bootstrap_ctx, blobrepo);
                    async move {
                        cache_warmup::cache_warmup(&bootstrap_ctx, &blobrepo, warmup_params).await
                    }
                });
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
                repo_client_knobs,
            )?;

            if let Some(warmup) = warmup {
                info!(&logger, "Waiting for cache warmup to complete...");
                warmup
                    .await?
                    .with_context(|| format!("Performing cache warmup on {}", name))?;
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
    fn parse(matches: &MononokeMatches<'_>) -> Result<Self, Error> {
        let aliases = match matches.values_of(ARG_ALIASES) {
            Some(values) => values
                .into_iter()
                .map(extract_alias)
                .collect::<Result<HashMap<_, _>, _>>()?,
            None => HashMap::new(),
        };
        let aliases = Arc::new(aliases);

        let config_store = matches.config_store();

        let config_handle = match matches.value_of(ARG_LIVE_CONFIG) {
            Some(spec) => {
                config_store.get_config_handle_DEPRECATED(parse_config_spec_to_path(spec)?)
            }
            None => Ok(ConfigHandle::default()),
        };

        let config =
            config_handle.with_context(|| format!("While parsing --{}", ARG_LIVE_CONFIG))?;

        Ok(Self { aliases, config })
    }
}

async fn fastreplay<R: AsyncRead + Unpin>(
    opts: &ReplayOpts,
    reader: R,
    logger: &Logger,
    scuba: &MononokeScubaSampleBuilder,
    repos: &Arc<HashMap<String, FastReplayDispatcher>>,
    count: &Arc<AtomicU64>,
) -> Result<(), Error> {
    let mut reader = BufReader::new(reader).lines();

    loop {
        let load = count.load(Ordering::Relaxed);

        let config = opts.config.get();

        if load >= config.max_concurrency()?.get() {
            warn!(
                &logger,
                "Waiting for some requests to complete (load: {})...", load
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
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
                cloned!(logger, mut scuba, repos, opts.aliases, count, config);

                scuba.sampled(config.scuba_sampling_target()?);

                let task = async move {
                    defer!({
                        count.fetch_sub(1, Ordering::Relaxed);
                    });

                    match dispatch(&repos, &aliases, config.skipped_repos(), &req, scuba).await {
                        Ok(DispatchOutcome::Processed) => {
                            STATS::processed.add_value(1);
                        }
                        Ok(DispatchOutcome::Skipped) => {
                            STATS::skipped.add_value(1);
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
    matches: &MononokeMatches<'a>,
    logger: &Logger,
    service: &ReadyFlagService,
) -> Result<(), Error> {
    let scuba = matches.scuba_sample_builder();

    // Do this earlier to show errors before we bootstrap repositories
    let opts = ReplayOpts::parse(&matches)?;

    let repos = bootstrap_repositories(fb, &matches, &logger, &scuba).await?;
    let repos = Arc::new(repos);

    // Report that we're good to go.
    service.set_ready();

    // Start replaying
    let count = Arc::new(AtomicU64::new(0));

    match matches.as_ref().values_of_os(ARG_COMMAND) {
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
            child.wait().await?;
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
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }

    Ok(())
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeAppBuilder::new("Mononoke Local Replay")
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
                .help("unused")
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
            Arg::with_name(ARG_MULTIPLEXEDBLOB_SAMPLING)
            .long(ARG_MULTIPLEXEDBLOB_SAMPLING)
            .takes_value(true)
            .required(false)
        )
        .arg(
            Arg::with_name(ARG_COMMAND)
                .help("Command to run to fetch traffic to replay (defaults to reading from stdin otherwise)")
                .takes_value(true)
                .multiple(true)
                .required(false)
        );

    let matches = app.get_matches(fb)?;

    let logger = matches.logger();
    let service = ReadyFlagService::new();

    let main = do_main(fb, &matches, logger, &service);

    cmdlib::helpers::block_execute(main, fb, "fastreplay", logger, &matches, service.clone())?;

    Ok(())
}
