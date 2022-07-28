/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "15000000"]
#![feature(map_first_last)]

use anyhow::anyhow;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blame::BlameRoot;
use blobrepo::BlobRepo;
use blobrepo_override::DangerousOverride;
use blobstore::StoreLoadable;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarksSubscription;
use bookmarks::Freshness;
use bytes::Bytes;
use cacheblob::dummy::DummyLease;
use cacheblob::InProcessLease;
use cacheblob::LeaseOps;
use changesets::deserialize_cs_entries;
use changesets::ChangesetEntry;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use cloned::cloned;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::args::RepoRequirement;
use cmdlib::helpers;
use context::CoreContext;
use context::SessionContainer;
use derived_data_manager::BonsaiDerivable as NewBonsaiDerivable;
use derived_data_utils::create_derive_graph_scuba_sample;
use derived_data_utils::derived_data_utils;
use derived_data_utils::derived_data_utils_for_config;
use derived_data_utils::DerivedUtils;
use derived_data_utils::ThinOut;
use derived_data_utils::DEFAULT_BACKFILLING_CONFIG_NAME;
use derived_data_utils::POSSIBLE_DERIVED_TYPES;
use executor_lib::BackgroundProcessExecutor;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use fbinit::FacebookInit;
use fsnodes::RootFsnodeId;
use futures::future;
use futures::future::try_join;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_stats::TimedFutureExt;
use futures_stats::TimedTryFutureExt;
use mononoke_api_types::InnerRepo;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use once_cell::sync::OnceCell;
use repo_factory::RepoFactoryBuilder;
use scuba_ext::MononokeScubaSampleBuilder;
use skiplist::SkiplistIndex;
use slog::info;
use slog::Logger;
use stats::prelude::*;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;
use time_ext::DurationExt;
use topo_sort::sort_topological;
use tunables::tunables;

mod commit_discovery;
mod regenerate;
mod slice;
mod validation;
mod warmup;

use commit_discovery::CommitDiscoveryOptions;

define_stats! {
    prefix = "mononoke.derived_data";
    oldest_underived_secs: dynamic_singleton_counter("{}.oldest_underived_secs", (reponame: String)),
    derivation_time_ms: dynamic_timeseries("{}.derivation_time_ms", (reponame: String); Average, Sum),
    derivation_idle_time_ms: dynamic_timeseries("{}.idle_time_ms", (reponame: String); Sum),
}

const ARG_ALL_TYPES: &str = "all-types";
const ARG_DERIVED_DATA_TYPE: &str = "derived-data-type";
const ARG_SKIP: &str = "skip-changesets";
const ARG_LIMIT: &str = "limit";
const ARG_REGENERATE: &str = "regenerate";
const ARG_PREFETCHED_COMMITS_PATH: &str = "prefetched-commits-path";
const ARG_CHANGESET: &str = "changeset";
const ARG_USE_SHARED_LEASES: &str = "use-shared-leases";
const ARG_STOP_ON_IDLE: &str = "stop-on-idle";
const ARG_BATCHED: &str = "batched";
const ARG_BATCH_SIZE: &str = "batch-size";
const ARG_PARALLEL: &str = "parallel";
const ARG_SLICED: &str = "sliced";
const ARG_SLICE_SIZE: &str = "slice-size";
const ARG_BACKFILL: &str = "backfill";
const ARG_GAP_SIZE: &str = "gap-size";
const ARG_JSON: &str = "json";
const ARG_VALIDATE_CHUNK_SIZE: &str = "validate-chunk-size";
const ARG_BACKFILL_CONFIG_NAME: &str = "backfill-config-name";

const SUBCOMMAND_BACKFILL: &str = "backfill";
const SUBCOMMAND_BACKFILL_ALL: &str = "backfill-all";
const SUBCOMMAND_BENCHMARK: &str = "benchmark";
const SUBCOMMAND_TAIL: &str = "tail";
const SUBCOMMAND_SINGLE: &str = "single";
const SUBCOMMAND_VALIDATE: &str = "validate";

const DEFAULT_BATCH_SIZE_STR: &str = "128";
const DEFAULT_SLICE_SIZE_STR: &str = "20000";
const DEFAULT_VALIDATE_CHUNK_SIZE: &str = "10000";
const SLEEP_TIME: u64 = 250;

/// Derived data types that are permitted to access redacted files. This list
/// should be limited to those data types that need access to the content of
/// redacted files in order to compute their data, and will not leak redacted
/// data; for example, derived data types that compute hashes of file
/// contents that form part of a Merkle tree, and thus need to have correct
/// hashes for file content.
const UNREDACTED_TYPES: &[&str] = &[
    // Fsnodes need access to redacted file contents to compute SHA-1 and
    // SHA-256 hashes of the file content, which form part of the fsnode
    // tree hashes. Redacted content is only hashed, and so cannot be
    // discovered via the fsnode tree.
    RootFsnodeId::NAME,
    // Blame does not contain any content of the file itself
    BlameRoot::NAME,
];

async fn open_repo_maybe_unredacted<RepoType>(
    fb: FacebookInit,
    logger: &Logger,
    matches: &MononokeMatches<'_>,
    data_types: &[impl AsRef<str>],
    repo_name: String,
) -> Result<RepoType>
where
    RepoType: for<'builder> facet::AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
{
    let mut unredacted = false;
    for data_type in data_types {
        unredacted |= UNREDACTED_TYPES.contains(&data_type.as_ref());
    }

    if unredacted {
        args::open_repo_by_name_unredacted(fb, logger, matches, repo_name).await
    } else {
        args::open_repo_by_name(fb, logger, matches, repo_name).await
    }
}

const SM_SERVICE_SCOPE: &str = "global";
const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;

/// Struct representing the Derived Data BP.
pub struct DerivedDataProcess {
    matches: Arc<MononokeMatches<'static>>,
    fb: FacebookInit,
}

impl DerivedDataProcess {
    fn new(fb: FacebookInit) -> Result<Self> {
        let app = args::MononokeAppBuilder::new("Utility to work with bonsai derived data")
            .with_advanced_args_hidden()
            .with_fb303_args()
            .with_repo_required(RepoRequirement::AtLeastOne)
            .with_dynamic_repos()
            .with_scuba_logging_args()
            .build()
            .about("Utility to work with bonsai derived data")
            .subcommand(
                SubCommand::with_name(SUBCOMMAND_BACKFILL)
                    .about("backfill derived data for public commits")
                    .arg(
                        Arg::with_name(ARG_DERIVED_DATA_TYPE)
                            .required(true)
                            .index(1)
                            .possible_values(POSSIBLE_DERIVED_TYPES)
                            .help("derived data type for which backfill will be run"),
                    )
                    .arg(
                        Arg::with_name(ARG_SKIP)
                            .long(ARG_SKIP)
                            .takes_value(true)
                            .help("skip this number of changesets"),
                    )
                    .arg(
                        Arg::with_name(ARG_LIMIT)
                            .long(ARG_LIMIT)
                            .takes_value(true)
                            .help("backfill at most this number of changesets"),
                    )
                    .arg(
                        Arg::with_name(ARG_REGENERATE)
                            .long(ARG_REGENERATE)
                            .help("regenerate derivations even if mapping contains changeset"),
                    )
                    .arg(
                        Arg::with_name(ARG_PREFETCHED_COMMITS_PATH)
                            .long(ARG_PREFETCHED_COMMITS_PATH)
                            .takes_value(true)
                            .required(false)
                            .help("a file with a list of bonsai changesets to backfill"),
                    )
                    .arg(
                        Arg::with_name(ARG_BATCH_SIZE)
                            .long(ARG_BATCH_SIZE)
                            .default_value(DEFAULT_BATCH_SIZE_STR)
                            .help("number of changesets in each derivation batch"),
                    )
                    .arg(
                        Arg::with_name(ARG_PARALLEL)
                            .long(ARG_PARALLEL)
                            .help("derive commits within a batch in parallel"),
                    )
                    .arg(
                        Arg::with_name(ARG_GAP_SIZE)
                            .long(ARG_GAP_SIZE)
                            .takes_value(true)
                            .help("size of gap to leave in derived data types that support gaps"),
                    )
                    .arg(
                        Arg::with_name(ARG_BACKFILL_CONFIG_NAME)
                            .long(ARG_BACKFILL_CONFIG_NAME)
                            .help("sets the name for backfilling derived data types config")
                            .takes_value(true),
                    ),
            )
            .subcommand(
                SubCommand::with_name(SUBCOMMAND_TAIL)
                    .about("tail public commits and fill derived data")
                    .arg(
                        Arg::with_name(ARG_DERIVED_DATA_TYPE)
                            .required(false)
                            .multiple(true)
                            .index(1)
                            .possible_values(POSSIBLE_DERIVED_TYPES)
                            // TODO(stash): T66492899 remove unused value
                            .help("Unused, will be deleted soon"),
                    )
                    .arg(
                        Arg::with_name(ARG_USE_SHARED_LEASES)
                            .long(ARG_USE_SHARED_LEASES)
                            .takes_value(false)
                            .required(false)
                            .help(concat!(
                                "By default the derived data tailer doesn't compete with ",
                                "other mononoke services for a derived data lease, so ",
                                "it will derive the data even if another mononoke service ",
                                "(e.g. mononoke_server, scs_server, ...) are already ",
                                "deriving it.\n\n",
                                "This flag disables this behaviour, meaning this command ",
                                "will compete for the derived data lease with other ",
                                "mononoke services and start deriving only if the lease ",
                                "is obtained.",
                            )),
                    )
                    .arg(
                        Arg::with_name(ARG_STOP_ON_IDLE)
                            .long(ARG_STOP_ON_IDLE)
                            .help("Stop tailing or backfilling when there is nothing left"),
                    )
                    .arg(
                        Arg::with_name(ARG_BATCHED)
                            .long(ARG_BATCHED)
                            .takes_value(false)
                            .required(false)
                            .help("Use batched deriver instead of calling `::derive` periodically"),
                    )
                    .arg(
                        Arg::with_name(ARG_BATCH_SIZE)
                            .long(ARG_BATCH_SIZE)
                            .default_value(DEFAULT_BATCH_SIZE_STR)
                            .help("number of changesets in each derivation batch"),
                    )
                    .arg(
                        Arg::with_name(ARG_PARALLEL)
                            .long(ARG_PARALLEL)
                            .help("derive commits within a batch in parallel"),
                    )
                    .arg(
                        Arg::with_name(ARG_BACKFILL)
                            .long(ARG_BACKFILL)
                            .help("also backfill derived data types configured for backfilling"),
                    )
                    .arg(
                        Arg::with_name(ARG_SLICED)
                            .long(ARG_SLICED)
                            .help("pre-slice repository using the skiplist index when backfilling"),
                    )
                    .arg(
                        Arg::with_name(ARG_SLICE_SIZE)
                            .long(ARG_SLICE_SIZE)
                            .default_value(DEFAULT_SLICE_SIZE_STR)
                            .help("number of generations to include in each generation slice"),
                    )
                    .arg(
                        Arg::with_name(ARG_GAP_SIZE)
                            .long(ARG_GAP_SIZE)
                            .takes_value(true)
                            .help("size of gap to leave in derived data types that support gaps"),
                    )
                    .arg(
                        Arg::with_name(ARG_BACKFILL_CONFIG_NAME)
                            .long(ARG_BACKFILL_CONFIG_NAME)
                            .help("sets the name for backfilling derived data types config")
                            .takes_value(true),
                    ),
            )
            .subcommand(
                SubCommand::with_name(SUBCOMMAND_SINGLE)
                    .about("backfill single changeset (mainly for performance testing purposes)")
                    .arg(
                        Arg::with_name(ARG_ALL_TYPES)
                            .long(ARG_ALL_TYPES)
                            .required(false)
                            .takes_value(false)
                            .help("derive all derived data types enabled for this repo"),
                    )
                    .arg(
                        Arg::with_name(ARG_CHANGESET)
                            .required(true)
                            .index(1)
                            .help("changeset by {hg|bonsai} hash or bookmark"),
                    )
                    .arg(
                        Arg::with_name(ARG_DERIVED_DATA_TYPE)
                            .required(false)
                            .index(2)
                            .multiple(true)
                            .conflicts_with(ARG_ALL_TYPES)
                            .possible_values(POSSIBLE_DERIVED_TYPES)
                            .help("derived data type for which backfill will be run"),
                    ),
            )
            .subcommand(
                SubCommand::with_name(SUBCOMMAND_BACKFILL_ALL)
                    .about("backfill all/many derived data types at once")
                    .arg(
                        Arg::with_name(ARG_DERIVED_DATA_TYPE)
                            .conflicts_with(ARG_ALL_TYPES)
                            .possible_values(POSSIBLE_DERIVED_TYPES)
                            .required(false)
                            .takes_value(true)
                            .multiple(true)
                            .help(concat!(
                                "derived data type for which backfill will be run, ",
                                "all enabled and backfilling types if not specified",
                            )),
                    )
                    .arg(
                        Arg::with_name(ARG_ALL_TYPES)
                            .long(ARG_ALL_TYPES)
                            .required(false)
                            .takes_value(false)
                            .help("derive all derived data types enabled for this repo"),
                    )
                    .arg(
                        Arg::with_name(ARG_BATCH_SIZE)
                            .long(ARG_BATCH_SIZE)
                            .default_value(DEFAULT_BATCH_SIZE_STR)
                            .help("number of changesets in each derivation batch"),
                    )
                    .arg(
                        Arg::with_name(ARG_PARALLEL)
                            .long(ARG_PARALLEL)
                            .help("derive commits within a batch in parallel"),
                    )
                    .arg(Arg::with_name(ARG_SLICED).long(ARG_SLICED).help(
                        "pre-slice repository into generation slices using the skiplist index",
                    ))
                    .arg(
                        Arg::with_name(ARG_SLICE_SIZE)
                            .long(ARG_SLICE_SIZE)
                            .default_value(DEFAULT_SLICE_SIZE_STR)
                            .help("number of generations to include in each generation slice"),
                    )
                    .arg(
                        Arg::with_name(ARG_GAP_SIZE)
                            .long(ARG_GAP_SIZE)
                            .takes_value(true)
                            .help("size of gap to leave in derived data types that support gaps"),
                    )
                    .arg(
                        Arg::with_name(ARG_BACKFILL_CONFIG_NAME)
                            .long(ARG_BACKFILL_CONFIG_NAME)
                            .help("sets the name for backfilling derived data types config")
                            .takes_value(true),
                    ),
            )
            .subcommand(
                regenerate::DeriveOptions::add_opts(
                    commit_discovery::CommitDiscoveryOptions::add_opts(
                        SubCommand::with_name(SUBCOMMAND_BENCHMARK)
                            .about("benchmark derivation of a list of commits")
                            .long_about(
                                "note that this command WILL DERIVE data and save it to storage",
                            ),
                    ),
                )
                .arg(
                    Arg::with_name(ARG_DERIVED_DATA_TYPE)
                        .required(true)
                        .index(1)
                        .multiple(true)
                        .conflicts_with(ARG_ALL_TYPES)
                        .possible_values(POSSIBLE_DERIVED_TYPES)
                        .help("derived data type for which backfill will be run"),
                )
                .arg(
                    Arg::with_name(ARG_ALL_TYPES)
                        .long(ARG_ALL_TYPES)
                        .required(false)
                        .takes_value(false)
                        .help("derive all derived data types enabled for this repo"),
                )
                .arg(
                    Arg::with_name(ARG_JSON)
                        .long(ARG_JSON)
                        .required(false)
                        .takes_value(false)
                        .help("Print result in json format"),
                ),
            )
            .subcommand(
                regenerate::DeriveOptions::add_opts(
                    commit_discovery::CommitDiscoveryOptions::add_opts(
                        SubCommand::with_name(SUBCOMMAND_VALIDATE)
                            .about(
                                "rederive the commits and make sure they are saved to the storage",
                            )
                            .long_about("this command won't write anything new to the storage"),
                    ),
                )
                .arg(
                    Arg::with_name(ARG_DERIVED_DATA_TYPE)
                        .required(true)
                        .index(1)
                        .multiple(true)
                        .conflicts_with(ARG_ALL_TYPES)
                        .possible_values(POSSIBLE_DERIVED_TYPES)
                        .help("derived data type for which backfill will be run"),
                )
                .arg(
                    Arg::with_name(ARG_VALIDATE_CHUNK_SIZE)
                        .long(ARG_VALIDATE_CHUNK_SIZE)
                        .default_value(DEFAULT_VALIDATE_CHUNK_SIZE)
                        .help("how many commits to validate at once."),
                )
                .arg(
                    Arg::with_name(ARG_JSON)
                        .long(ARG_JSON)
                        .required(false)
                        .takes_value(false)
                        .help("Print result in json format"),
                ),
            );
        let matches = Arc::new(app.get_matches(fb)?);
        Ok(Self { matches, fb })
    }
}

#[async_trait]
impl RepoShardedProcess for DerivedDataProcess {
    async fn setup(&self, repo_name: &str) -> anyhow::Result<Arc<dyn RepoShardedProcessExecutor>> {
        info!(
            self.matches.logger(),
            "Setting up derived data command for repo {}", repo_name
        );
        let executor = DerivedDataProcessExecutor::new(
            self.fb,
            Arc::clone(&self.matches),
            repo_name.to_string(),
        );
        info!(
            self.matches.logger(),
            "Completed derived data command setup for repo {}", repo_name
        );
        Ok(Arc::new(executor))
    }
}

/// Struct representing the execution of the Derived Data
/// BP over the context of a provided repo.
pub struct DerivedDataProcessExecutor {
    fb: FacebookInit,
    matches: Arc<MononokeMatches<'static>>,
    ctx: CoreContext,
    cancellation_requested: Arc<AtomicBool>,
    repo_name: String,
}

impl DerivedDataProcessExecutor {
    fn new(fb: FacebookInit, matches: Arc<MononokeMatches<'static>>, repo_name: String) -> Self {
        let mut scuba_sample_builder = matches.scuba_sample_builder();
        scuba_sample_builder.add("reponame", repo_name.to_string());
        let ctx = create_ctx(fb, matches.logger(), scuba_sample_builder, &matches)
            .clone_with_repo_name(&repo_name);
        Self {
            fb,
            matches,
            ctx,
            repo_name,
            cancellation_requested: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[async_trait]
impl RepoShardedProcessExecutor for DerivedDataProcessExecutor {
    async fn execute(&self) -> anyhow::Result<()> {
        info!(
            self.ctx.logger(),
            "Initiating derived data command execution for repo {}", &self.repo_name,
        );
        run_subcmd(
            self.fb,
            &self.ctx,
            self.ctx.logger(),
            &self.matches,
            self.repo_name.clone(),
            Arc::clone(&self.cancellation_requested),
        )
        .await
        .with_context(|| {
            format!(
                "Error during derived data command execution for repo {}",
                &self.repo_name
            )
        })?;
        info!(
            self.ctx.logger(),
            "Finished derived data command execution for repo {}", &self.repo_name,
        );
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        info!(
            self.ctx.logger(),
            "Terminating derived data command execution for repo {}", &self.repo_name,
        );
        self.cancellation_requested.store(true, Ordering::Relaxed);
        Ok(())
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let process = DerivedDataProcess::new(fb)?;
    match process.matches.value_of("sharded-service-name") {
        Some(service_name) => {
            // The service name needs to be 'static to satisfy SM contract
            static SM_SERVICE_NAME: OnceCell<String> = OnceCell::new();
            let logger = process.matches.logger().clone();
            let matches = Arc::clone(&process.matches);
            let mut executor = BackgroundProcessExecutor::new(
                process.fb,
                process.matches.runtime().clone(),
                &logger,
                SM_SERVICE_NAME.get_or_init(|| service_name.to_string()),
                SM_SERVICE_SCOPE,
                SM_CLEANUP_TIMEOUT_SECS,
                Arc::new(process),
            )?;
            helpers::block_execute(
                executor.block_and_execute(&logger),
                fb,
                &std::env::var("TW_JOB_NAME")
                    .unwrap_or_else(|_| "backfill_derived_data".to_string()),
                matches.logger(),
                &matches,
                cmdlib::monitoring::AliveService,
            )
        }
        None => {
            let process = Arc::new(process);
            let all_repo_derivation = stream::iter(
                args::resolve_repos(process.matches.config_store(), &process.matches)?
                    .into_iter()
                    .map(|repo| {
                        let process = Arc::clone(&process);
                        async move {
                            let executor = process.setup(&repo.name).await?;
                            executor.execute().await
                        }
                    }),
            )
            // Each item is a repo. Don't need to derive data for more than 10 repos
            // at a time when executing in non-sharded setting.
            .buffer_unordered(10)
            .try_collect::<Vec<_>>();

            helpers::block_execute(
                all_repo_derivation,
                fb,
                &std::env::var("TW_JOB_NAME")
                    .unwrap_or_else(|_| "backfill_derived_data".to_string()),
                process.matches.logger(),
                &process.matches,
                cmdlib::monitoring::AliveService,
            )?;
            Ok(())
        }
    }
}

fn create_ctx<'a>(
    fb: FacebookInit,
    logger: &Logger,
    scuba_sample_builder: MononokeScubaSampleBuilder,
    matches: &'a MononokeMatches<'a>,
) -> CoreContext {
    let mut ctx =
        SessionContainer::new_with_defaults(fb).new_context(logger.clone(), scuba_sample_builder);
    match matches.subcommand() {
        (SUBCOMMAND_BACKFILL_ALL, _) | (SUBCOMMAND_BACKFILL, _) => {
            ctx.session_mut()
                .override_session_class(context::SessionClass::Background);
        }
        _ => {}
    };

    ctx
}

async fn run_subcmd<'a>(
    fb: FacebookInit,
    ctx: &CoreContext,
    logger: &Logger,
    matches: &'a MononokeMatches<'a>,
    repo_name: String,
    cancellation_requested: Arc<AtomicBool>,
) -> Result<()> {
    match matches.subcommand() {
        (SUBCOMMAND_BACKFILL_ALL, Some(sub_m)) => {
            let repo: InnerRepo =
                args::open_repo_by_name_unredacted(fb, logger, matches, repo_name).await?;

            let backfill_config_name = sub_m
                .value_of(ARG_BACKFILL_CONFIG_NAME)
                .unwrap_or(DEFAULT_BACKFILLING_CONFIG_NAME);

            let derived_data_types = sub_m.values_of(ARG_DERIVED_DATA_TYPE).map_or_else(
                || {
                    let enabled_types = repo.blob_repo.get_active_derived_data_types_config();
                    if let Some(backfill_config) = repo
                        .blob_repo
                        .get_derived_data_types_config(backfill_config_name)
                    {
                        &enabled_types.types | &backfill_config.types
                    } else {
                        enabled_types.types.clone()
                    }
                },
                |names| names.map(ToString::to_string).collect(),
            );

            let batch_size = sub_m
                .value_of(ARG_BATCH_SIZE)
                .expect("batch-size must be set")
                .parse::<usize>()?;
            let parallel = sub_m.is_present(ARG_PARALLEL);
            let slice_size = if sub_m.is_present(ARG_SLICED) {
                Some(
                    sub_m
                        .value_of(ARG_SLICE_SIZE)
                        .expect("slice-size must be set")
                        .parse::<u64>()?,
                )
            } else {
                None
            };
            let gap_size = sub_m
                .value_of(ARG_GAP_SIZE)
                .map(str::parse::<usize>)
                .transpose()?;

            subcommand_backfill_all(
                ctx,
                &repo,
                derived_data_types,
                slice_size,
                batch_size,
                parallel,
                gap_size,
                backfill_config_name,
            )
            .await
        }
        (SUBCOMMAND_BACKFILL, Some(sub_m)) => {
            let derived_data_type = sub_m
                .value_of(ARG_DERIVED_DATA_TYPE)
                .ok_or_else(|| format_err!("missing required argument: {}", ARG_DERIVED_DATA_TYPE))?
                .to_string();

            let prefetched_commits_path = sub_m
                .value_of(ARG_PREFETCHED_COMMITS_PATH)
                .ok_or_else(|| {
                    format_err!("missing required argument: {}", ARG_PREFETCHED_COMMITS_PATH)
                })?
                .to_string();

            let regenerate = sub_m.is_present(ARG_REGENERATE);

            let skip = sub_m
                .value_of(ARG_SKIP)
                .map(|skip| skip.parse::<usize>())
                .transpose()
                .map(|skip| skip.unwrap_or(0))?;

            let maybe_limit = sub_m
                .value_of(ARG_LIMIT)
                .map(|limit| limit.parse::<usize>())
                .transpose()?;

            let repo: InnerRepo =
                open_repo_maybe_unredacted(fb, logger, matches, &[&derived_data_type], repo_name)
                    .await?;

            info!(
                ctx.logger(),
                "reading all changesets for: {:?}",
                repo.blob_repo.get_repoid()
            );
            let mut changesets = parse_serialized_commits(prefetched_commits_path)?;
            changesets.sort_by_key(|cs_entry| cs_entry.gen);

            let iter = changesets.into_iter().skip(skip);
            let changesets = match maybe_limit {
                Some(limit) => iter.take(limit).map(|entry| entry.cs_id).collect(),
                None => iter.map(|entry| entry.cs_id).collect(),
            };

            let parallel = sub_m.is_present(ARG_PARALLEL);
            let batch_size = sub_m
                .value_of(ARG_BATCH_SIZE)
                .expect("batch-size must be set")
                .parse::<usize>()?;
            let gap_size = sub_m
                .value_of(ARG_GAP_SIZE)
                .map(str::parse::<usize>)
                .transpose()?;

            let backfill_config_name = sub_m
                .value_of(ARG_BACKFILL_CONFIG_NAME)
                .unwrap_or(DEFAULT_BACKFILLING_CONFIG_NAME);

            subcommand_backfill(
                ctx,
                &repo,
                derived_data_type.as_str(),
                regenerate,
                parallel,
                batch_size,
                gap_size,
                changesets,
                backfill_config_name,
            )
            .await
        }
        (SUBCOMMAND_TAIL, Some(sub_m)) => {
            let config_store = matches.config_store();
            let use_shared_leases = sub_m.is_present(ARG_USE_SHARED_LEASES);
            let stop_on_idle = sub_m.is_present(ARG_STOP_ON_IDLE);
            let batched = sub_m.is_present(ARG_BATCHED);
            let parallel = sub_m.is_present(ARG_PARALLEL);
            let batch_size = if batched {
                Some(
                    sub_m
                        .value_of(ARG_BATCH_SIZE)
                        .expect("batch-size must be set")
                        .parse::<usize>()?,
                )
            } else {
                None
            };
            let backfill = sub_m.is_present(ARG_BACKFILL);
            let slice_size = if sub_m.is_present(ARG_SLICED) {
                Some(
                    sub_m
                        .value_of(ARG_SLICE_SIZE)
                        .expect("slice-size must be set")
                        .parse::<u64>()?,
                )
            } else {
                None
            };
            let gap_size = sub_m
                .value_of(ARG_GAP_SIZE)
                .map(str::parse::<usize>)
                .transpose()?;

            let backfill_config_name = sub_m
                .value_of(ARG_BACKFILL_CONFIG_NAME)
                .unwrap_or(DEFAULT_BACKFILLING_CONFIG_NAME);

            let resolved_repo = args::resolve_repo_by_name(config_store, matches, &repo_name)?;

            let (blob_repo, skiplist) = if backfill {
                let inner: InnerRepo =
                    args::open_repo_by_id(fb, logger, matches, resolved_repo.id).await?;
                (inner.blob_repo.clone(), Some(inner.skiplist_index))
            } else {
                (
                    args::open_repo_by_id(fb, logger, matches, resolved_repo.id).await?,
                    None,
                )
            };

            subcommand_tail(
                ctx,
                blob_repo,
                skiplist,
                use_shared_leases,
                stop_on_idle,
                batch_size,
                parallel,
                gap_size,
                backfill,
                slice_size,
                backfill_config_name,
                cancellation_requested,
            )
            .await
        }
        (SUBCOMMAND_SINGLE, Some(sub_m)) => {
            let hash_or_bookmark = sub_m
                .value_of_lossy(ARG_CHANGESET)
                .ok_or_else(|| format_err!("missing required argument: {}", ARG_CHANGESET))?
                .to_string();
            let (repo, types) =
                parse_repo_and_derived_data_types(fb, logger, matches, sub_m, repo_name).await?;
            let csid = helpers::csid_resolve(ctx, repo.clone(), hash_or_bookmark).await?;
            subcommand_single(ctx, &repo, csid, types).await
        }
        (SUBCOMMAND_BENCHMARK, Some(sub_m)) => {
            let (repo, types) =
                parse_repo_and_derived_data_types(fb, logger, matches, sub_m, repo_name).await?;
            let csids = CommitDiscoveryOptions::from_matches(ctx, &repo, sub_m)
                .await?
                .get_commits();

            let opts = regenerate::DeriveOptions::from_matches(sub_m)?;

            let stats =
                regenerate::regenerate_derived_data(ctx, &repo, csids, types, &opts).await?;

            println!("Building derive graph took {:?}", stats.build_derive_graph);
            println!("Derivation took {:?}", stats.derivation);

            Ok(())
        }
        (SUBCOMMAND_VALIDATE, Some(sub_m)) => {
            crate::validation::validate(ctx, matches, sub_m, repo_name).await
        }
        (name, _) => Err(format_err!("unhandled subcommand: {}", name)),
    }
}

async fn parse_repo_and_derived_data_types(
    fb: FacebookInit,
    logger: &Logger,
    matches: &MononokeMatches<'_>,
    sub_m: &ArgMatches<'_>,
    repo_name: String,
) -> Result<(BlobRepo, Vec<String>)> {
    let all = sub_m.is_present(ARG_ALL_TYPES);
    let derived_data_types = sub_m.values_of(ARG_DERIVED_DATA_TYPE);
    let (repo, types): (_, Vec<String>) = match (all, derived_data_types) {
        (true, None) => {
            let repo: BlobRepo = args::open_repo_unredacted(fb, logger, matches).await?;
            let types = repo
                .get_active_derived_data_types_config()
                .types
                .iter()
                .cloned()
                .collect();
            (repo, types)
        }
        (false, Some(derived_data_types)) => {
            let derived_data_types = derived_data_types
                .into_iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>();
            let repo: BlobRepo =
                open_repo_maybe_unredacted(fb, logger, matches, &derived_data_types, repo_name)
                    .await?;
            (repo, derived_data_types)
        }
        (true, Some(_)) => {
            return Err(format_err!(
                "{} and {} can't be specified",
                ARG_ALL_TYPES,
                ARG_DERIVED_DATA_TYPE
            ));
        }
        (false, None) => {
            return Err(format_err!(
                "{} or {} should be specified",
                ARG_ALL_TYPES,
                ARG_DERIVED_DATA_TYPE
            ));
        }
    };

    Ok((repo, types))
}

fn parse_serialized_commits<P: AsRef<Path>>(file: P) -> Result<Vec<ChangesetEntry>> {
    let data = fs::read(file)?;
    deserialize_cs_entries(&Bytes::from(data))
}

async fn subcommand_backfill_all(
    ctx: &CoreContext,
    repo: &InnerRepo,
    derived_data_types: HashSet<String>,
    slice_size: Option<u64>,
    batch_size: usize,
    parallel: bool,
    gap_size: Option<usize>,
    config_name: &str,
) -> Result<()> {
    info!(ctx.logger(), "derived data types: {:?}", derived_data_types);
    let derivers = derived_data_types
        .iter()
        .map(|name| {
            derived_data_utils_for_config(ctx.fb, &repo.blob_repo, name.as_str(), config_name)
        })
        .collect::<Result<Vec<_>, _>>()?;

    let heads = get_most_recent_heads(ctx, &repo.blob_repo).await?;
    backfill_heads(
        ctx,
        &repo.blob_repo,
        Some(&repo.skiplist_index),
        derivers.as_ref(),
        heads,
        slice_size,
        batch_size,
        parallel,
        gap_size,
    )
    .await
}

async fn backfill_heads(
    ctx: &CoreContext,
    repo: &BlobRepo,
    skiplist_index: Option<&SkiplistIndex>,
    derivers: &[Arc<dyn DerivedUtils>],
    heads: Vec<ChangesetId>,
    slice_size: Option<u64>,
    batch_size: usize,
    parallel: bool,
    gap_size: Option<usize>,
) -> Result<()> {
    if let (Some(skiplist_index), Some(slice_size)) = (skiplist_index, slice_size) {
        let (count, slices) =
            slice::slice_repository(ctx, repo, skiplist_index, derivers, heads, slice_size).await?;
        for (index, (id, slice_heads)) in slices.enumerate() {
            info!(
                ctx.logger(),
                "Deriving slice {} ({}/{}) with {} heads",
                id,
                index + 1,
                count,
                slice_heads.len()
            );
            tail_batch_iteration(
                ctx,
                repo,
                derivers,
                slice_heads,
                batch_size,
                parallel,
                gap_size,
            )
            .await?;
        }
    } else {
        info!(ctx.logger(), "Deriving {} heads", heads.len());
        tail_batch_iteration(ctx, repo, derivers, heads, batch_size, parallel, gap_size).await?;
    }
    Ok(())
}

fn truncate_duration(duration: Duration) -> Duration {
    Duration::from_secs(duration.as_secs())
}

async fn get_batch_ctx(ctx: &CoreContext, limit_qps: bool) -> CoreContext {
    if limit_qps {
        // create new context so each derivation batch has its own trace
        // and is rate-limited
        let session = SessionContainer::builder(ctx.fb)
            .blobstore_maybe_read_qps_limiter(tunables().get_backfill_read_qps())
            .await
            .blobstore_maybe_write_qps_limiter(tunables().get_backfill_write_qps())
            .await
            .build();
        session.new_context(
            ctx.logger().clone(),
            MononokeScubaSampleBuilder::with_discard(),
        )
    } else {
        ctx.clone()
    }
}

async fn subcommand_backfill(
    ctx: &CoreContext,
    repo: &InnerRepo,
    derived_data_type: &str,
    regenerate: bool,
    parallel: bool,
    batch_size: usize,
    gap_size: Option<usize>,
    changesets: Vec<ChangesetId>,
    config_name: &str,
) -> Result<()> {
    let derived_utils =
        &derived_data_utils_for_config(ctx.fb, &repo.blob_repo, derived_data_type, config_name)?;

    info!(
        ctx.logger(),
        "starting deriving data for {} changesets",
        changesets.len()
    );

    let total_count = changesets.len();
    let mut generated_count = 0usize;
    let mut skipped_count = 0usize;
    let mut total_duration = Duration::from_secs(0);

    if regenerate {
        derived_utils.regenerate(&changesets);
    }

    for chunk in changesets.chunks(batch_size) {
        info!(
            ctx.logger(),
            "starting batch of {} from {}",
            chunk.len(),
            chunk.first().unwrap()
        );
        let (stats, chunk_size) = async {
            let chunk = derived_utils
                .pending(ctx.clone(), repo.blob_repo.clone(), chunk.to_vec())
                .await?;
            let chunk_size = chunk.len();

            warmup::warmup(ctx, &repo.blob_repo, derived_data_type, &chunk).await?;
            info!(ctx.logger(), "warmup of {} changesets complete", chunk_size);

            derived_utils
                .backfill_batch_dangerous(
                    get_batch_ctx(ctx, parallel || gap_size.is_some()).await,
                    repo.blob_repo.clone(),
                    chunk,
                    parallel,
                    gap_size,
                )
                .await?;
            Result::<_>::Ok(chunk_size)
        }
        .timed()
        .await;

        let chunk_size = chunk_size?;
        generated_count += chunk_size;
        let elapsed = stats.completion_time;
        total_duration += elapsed;

        if chunk_size < chunk.len() {
            info!(
                ctx.logger(),
                "skipped {} changesets as they were already generated",
                chunk.len() - chunk_size,
            );
            skipped_count += chunk.len() - chunk_size;
        }
        if generated_count != 0 {
            let generated = generated_count as f32;
            let total = (total_count - skipped_count) as f32;
            let estimate = total_duration.mul_f32((total - generated) / generated);

            info!(
                ctx.logger(),
                "{}/{} ({} in {}) estimate:{} speed:{:.2}/s overall_speed:{:.2}/s",
                generated,
                total_count - skipped_count,
                chunk_size,
                humantime::format_duration(truncate_duration(elapsed)),
                humantime::format_duration(truncate_duration(estimate)),
                chunk_size as f32 / elapsed.as_secs() as f32,
                generated / total_duration.as_secs() as f32,
            );
        }
    }
    Ok(())
}

async fn subcommand_tail(
    ctx: &CoreContext,
    repo: BlobRepo,
    skiplist_index: Option<Arc<SkiplistIndex>>,
    use_shared_leases: bool,
    stop_on_idle: bool,
    batch_size: Option<usize>,
    parallel: bool,
    gap_size: Option<usize>,
    mut backfill: bool,
    slice_size: Option<u64>,
    config_name: &str,
    cancellation_requested: Arc<AtomicBool>,
) -> Result<()> {
    if backfill && batch_size == None {
        return Err(anyhow!("tail --backfill requires --batched"));
    }

    let repo = if use_shared_leases {
        // "shared" leases are the default - so we don't need to do anything.
        repo
    } else {
        // We use a separate derive data lease for derived_data_tailer
        // so that it could continue deriving even if all other services are failing.
        // Note that we could've removed the lease completely, but that would've been
        // problematic for unodes. Blame, fastlog and deleted_file_manifest all want
        // to derive unodes, so with no leases at all we'd derive unodes 4 times.
        let lease = InProcessLease::new();
        repo.dangerous_override(|_| Arc::new(lease) as Arc<dyn LeaseOps>)
    };
    let repo = &repo;

    let active_derived_data_config = repo.get_active_derived_data_types_config();

    let tail_derivers: Vec<Arc<dyn DerivedUtils>> = active_derived_data_config
        .types
        .iter()
        .map(|name| derived_data_utils(ctx.fb, repo, name))
        .collect::<Result<_>>()?;
    slog::info!(
        ctx.logger(),
        "[{}] tailing derived data: {:?}",
        repo.name(),
        tail_derivers
            .iter()
            .map(|d| d.name())
            .collect::<BTreeSet<_>>(),
    );

    let mut bookmarks_subscription = repo
        .bookmarks()
        .create_subscription(ctx, Freshness::MostRecent)
        .await
        .context("Error creating bookmarks subscription")?;

    let backfill_derivers: Vec<Arc<dyn DerivedUtils>> =
        if let Some(named_derived_data_config) = repo.get_derived_data_types_config(config_name) {
            if backfill {
                // Some backfilling types may depend on enabled types for their
                // derivation.  This means we need to include the appropriate
                // derivers for all types (enabled and backfilling).  Since the
                // enabled type will already have been derived, the deriver for
                // those types will just be used for mapping look-ups.  The
                // `derived_data_utils_for_config` function takes care of giving
                // us the right deriver type for the config.
                active_derived_data_config
                    .types
                    .union(&named_derived_data_config.types)
                    .map(|name| derived_data_utils_for_config(ctx.fb, repo, name, config_name))
                    .collect::<Result<_>>()?
            } else {
                Vec::new()
            }
        } else {
            backfill = false;
            Vec::new()
        };
    if backfill {
        slog::info!(
            ctx.logger(),
            "[{}] backfilling derived data: {:?}",
            repo.name(),
            backfill_derivers
                .iter()
                .map(|d| d.name())
                .collect::<BTreeSet<_>>(),
        );
    }

    // Before beginning, check if cancellation has been requested.
    if cancellation_requested.load(Ordering::Relaxed) {
        info!(ctx.logger(), "tail stopping due to cancellation request");
        return Ok(());
    }
    if let Some(batch_size) = batch_size {
        info!(ctx.logger(), "using batched deriver");

        let (sender, receiver) = tokio::sync::watch::channel(HashSet::new());

        let tail_loop = async move {
            cloned!(ctx, repo);
            tokio::spawn(async move {
                let mut derived_heads = HashSet::new();
                loop {
                    let heads_res = bookmarks_subscription.refresh(&ctx).await;
                    let heads = match heads_res {
                        Ok(()) => bookmarks_subscription
                            .bookmarks()
                            .iter()
                            .map(|(_, (cs_id, _))| *cs_id)
                            .collect::<HashSet<_>>(),
                        Err(e) => return Err::<(), _>(e),
                    };
                    let underived_heads = heads
                        .difference(&derived_heads)
                        .cloned()
                        .collect::<Vec<_>>();
                    if stop_on_idle && underived_heads.is_empty() {
                        info!(ctx.logger(), "tail stopping due to --stop-on-idle");
                        return Ok(());
                    }
                    tail_batch_iteration(
                        &ctx,
                        &repo,
                        &tail_derivers,
                        underived_heads,
                        batch_size,
                        parallel,
                        gap_size,
                    )
                    .await?;
                    let _ = sender.send(heads.clone());
                    derived_heads = heads;
                    // Before initiating next iteration, check if cancellation
                    // has been requested
                    if cancellation_requested.load(Ordering::Relaxed) {
                        info!(ctx.logger(), "tail stopping due to cancellation request");
                        return Ok(());
                    }
                }
            })
            .await?
        };

        let backfill_loop = async move {
            cloned!(ctx, repo);
            tokio::spawn(async move {
                if backfill {
                    let mut derived_heads = HashSet::new();
                    let mut receiver = tokio_stream::wrappers::WatchStream::new(receiver);
                    while let Some(heads) = receiver.next().await {
                        let underived_heads = heads
                            .difference(&derived_heads)
                            .cloned()
                            .collect::<Vec<_>>();
                        backfill_heads(
                            &ctx,
                            &repo,
                            skiplist_index.as_deref(),
                            &backfill_derivers,
                            underived_heads,
                            slice_size,
                            batch_size,
                            parallel,
                            gap_size,
                        )
                        .await?;
                        derived_heads = heads;
                    }
                    info!(ctx.logger(), "backfill stopping");
                }
                Ok::<_, Error>(())
            })
            .await?
        };

        try_join(tail_loop, backfill_loop).await?;
    } else {
        info!(ctx.logger(), "using simple deriver");
        loop {
            tail_one_iteration(ctx, repo, &tail_derivers, &mut bookmarks_subscription).await?;
            // Before initiating next iteration, check if cancellation
            // has been requested
            if cancellation_requested.load(Ordering::Relaxed) {
                info!(ctx.logger(), "tail stopping due to cancellation request");
                return Ok(());
            }
        }
    }
    Ok(())
}

async fn get_most_recent_heads(ctx: &CoreContext, repo: &BlobRepo) -> Result<Vec<ChangesetId>> {
    repo.bookmarks()
        .list(
            ctx.clone(),
            Freshness::MostRecent,
            &BookmarkPrefix::empty(),
            BookmarkKind::ALL_PUBLISHING,
            &BookmarkPagination::FromStart,
            std::u64::MAX,
        )
        .map_ok(|(_name, csid)| csid)
        .try_collect::<Vec<_>>()
        .await
}

async fn tail_batch_iteration(
    ctx: &CoreContext,
    repo: &BlobRepo,
    derive_utils: &[Arc<dyn DerivedUtils>],
    heads: Vec<ChangesetId>,
    batch_size: usize,
    parallel: bool,
    gap_size: Option<usize>,
) -> Result<()> {
    let derive_graph = derived_data_utils::build_derive_graph(
        ctx,
        repo,
        heads,
        derive_utils.to_vec(),
        batch_size,
        // This means that for 1000 commits it will inspect all changesets for underived data
        // after 1000 commits in 1000 * 1.5 commits, then 1000 in 1000 * 1.5 ^ 2 ... 1000 in 1000 * 1.5 ^ n
        ThinOut::new(1000.0, 1.5),
    )
    .await?;

    let size = derive_graph.size();
    if size == 0 {
        STATS::derivation_idle_time_ms.add_value(SLEEP_TIME as i64, (repo.name().to_string(),));
        tokio::time::sleep(Duration::from_millis(SLEEP_TIME)).await;
    } else {
        info!(ctx.logger(), "deriving data {}", size);
        // Find all the commits that we need to derive, and fetch gen number
        // so that we can sort them lexicographically
        let commits = derive_graph.commits();
        let cs_fetcher = &repo.get_changeset_fetcher();

        let mut commits = stream::iter(commits.into_iter().map(|cs_id| async move {
            let gen_num = cs_fetcher.get_generation_number(ctx.clone(), cs_id).await?;
            Result::<_, Error>::Ok((cs_id, gen_num))
        }))
        .buffer_unordered(100)
        .try_collect::<Vec<_>>()
        .await?;

        // We are using `bounded_traversal_dag` directly instead of `DeriveGraph::derive`
        // so we could use `warmup::warmup` on each node.
        let (stats, res) = bounded_traversal::bounded_traversal_dag(
            100,
            derive_graph,
            |node| {
                async move {
                    let deps = node.dependencies.clone();
                    Ok((node, deps))
                }
                .boxed()
            },
            move |node, _| {
                cloned!(ctx, repo);
                async move {
                    if let Some(deriver) = &node.deriver {
                        let mut scuba =
                            create_derive_graph_scuba_sample(&ctx, &node.csids, deriver.name());
                        let (stats, _) = warmup::warmup(&ctx, &repo, deriver.name(), &node.csids)
                            .try_timed()
                            .await?;
                        scuba.add_future_stats(&stats).log_with_msg("Warmup", None);
                        let timestamp = Instant::now();

                        let job = deriver
                            .backfill_batch_dangerous(
                                get_batch_ctx(&ctx, parallel || gap_size.is_some()).await,
                                repo.clone(),
                                node.csids.clone(),
                                parallel,
                                gap_size,
                            )
                            .try_timed();
                        let (stats, _) = tokio::spawn(job).await??;

                        if let (Some(first), Some(last)) = (node.csids.first(), node.csids.last()) {
                            slog::info!(
                                ctx.logger(),
                                "[{}:{}] count:{} time:{:.2?} start:{} end:{}",
                                deriver.name(),
                                node.id,
                                node.csids.len(),
                                timestamp.elapsed(),
                                first,
                                last
                            );
                            scuba
                                .add_future_stats(&stats)
                                .log_with_msg("Derived stack", None);
                        }
                    }
                    Result::<_>::Ok(())
                }
                .boxed()
            },
        )
        .try_timed()
        .await?;
        res.ok_or_else(|| anyhow!("derive graph contains a cycle"))?;

        // Log how long it took to derive all the data for all the commits
        commits.sort_by_key(|(_, gen)| *gen);
        let commits: Vec<_> = commits.into_iter().map(|(cs_id, _)| cs_id).collect();
        let mut scuba = create_derive_graph_scuba_sample(ctx, &commits, "all");
        scuba
            .add_future_stats(&stats)
            .log_with_msg("Derived stack", None);
    }

    Ok(())
}

async fn find_oldest_underived(
    ctx: &CoreContext,
    repo: &BlobRepo,
    derive: &dyn DerivedUtils,
    csids: Vec<ChangesetId>,
) -> Result<Option<BonsaiChangeset>> {
    let underived_ancestors = stream::iter(csids)
        .map(|csid| {
            Ok(async move {
                let underived = derive.find_underived(ctx, repo, csid).await?;
                let underived = sort_topological(&underived)
                    .ok_or_else(|| anyhow!("commit graph has cycles!"))?;
                // The first element is the first underived ancestor in
                // toposorted order.  Let's use it as a proxy for the oldest
                // underived commit.
                match underived.first() {
                    Some(csid) => Ok(Some(csid.load(ctx, repo.blobstore()).await?)),
                    None => Ok::<_, Error>(None),
                }
            })
        })
        .try_buffer_unordered(100)
        .try_collect::<Vec<_>>()
        .await?;
    Ok(underived_ancestors
        .into_iter()
        .flatten()
        .min_by_key(|bcs| *bcs.author_date()))
}

async fn tail_one_iteration(
    ctx: &CoreContext,
    repo: &BlobRepo,
    derive_utils: &[Arc<dyn DerivedUtils>],
    bookmarks_subscription: &mut Box<dyn BookmarksSubscription>,
) -> Result<()> {
    bookmarks_subscription
        .refresh(ctx)
        .await
        .context("failed refreshing bookmarks subscriptions")?;
    let heads = bookmarks_subscription
        .bookmarks()
        .iter()
        .map(|(_, (cs_id, _))| *cs_id)
        .collect::<Vec<_>>();

    // Find heads that needs derivation and find their oldest underived ancestor
    let find_pending_futs: Vec<_> = derive_utils
        .iter()
        .map({
            |derive| {
                let heads = heads.clone();
                async move {
                    let pending = derive.pending(ctx.clone(), (*repo).clone(), heads).await?;

                    let oldest_underived =
                        find_oldest_underived(ctx, repo, derive.as_ref(), pending.clone()).await?;
                    let now = DateTime::now();
                    let oldest_underived_age = oldest_underived.map_or(0, |oldest_underived| {
                        now.timestamp_secs() - oldest_underived.author_date().timestamp_secs()
                    });

                    Result::<_>::Ok((derive, pending, oldest_underived_age))
                }
            }
        })
        .collect();

    let pending = future::try_join_all(find_pending_futs).await?;

    // Log oldest underived ancestor to ods
    let mut oldest_underived_age = 0;
    for (_, _, cur_oldest_underived_age) in &pending {
        oldest_underived_age = ::std::cmp::max(oldest_underived_age, *cur_oldest_underived_age);
    }
    STATS::oldest_underived_secs.set_value(ctx.fb, oldest_underived_age, (repo.name().clone(),));

    let pending_futs = pending.into_iter().map(|(derive, pending, _)| {
        pending
            .into_iter()
            .map(|csid| derive.derive(ctx.clone(), (*repo).clone(), csid))
            .collect::<Vec<_>>()
    });

    let pending_futs: Vec<_> = pending_futs.flatten().collect();

    if pending_futs.is_empty() {
        STATS::derivation_idle_time_ms.add_value(SLEEP_TIME as i64, (repo.name().to_string(),));
        tokio::time::sleep(Duration::from_millis(SLEEP_TIME)).await;
        Ok(())
    } else {
        let count = pending_futs.len();
        info!(ctx.logger(), "found {} outdated heads", count);

        let (stats, res) = stream::iter(pending_futs)
            .buffered(1024)
            .try_for_each(|_: String| async { Ok(()) })
            .timed()
            .await;

        res?;
        info!(
            ctx.logger(),
            "derived data for {} heads in {:?}", count, stats.completion_time
        );
        STATS::derivation_time_ms.add_value(
            stats.completion_time.as_millis_unchecked() as i64,
            (repo.name().to_string(),),
        );
        Ok(())
    }
}

async fn subcommand_single(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csid: ChangesetId,
    derived_data_types: Vec<String>,
) -> Result<()> {
    let repo = repo.dangerous_override(|_| Arc::new(DummyLease {}) as Arc<dyn LeaseOps>);
    let mut derived_utils = vec![];
    for ty in derived_data_types {
        let utils = derived_data_utils(ctx.fb, &repo, ty)?;
        utils.regenerate(&[csid]);
        derived_utils.push(utils);
    }
    stream::iter(derived_utils)
        .map(Ok)
        .try_for_each_concurrent(100, |derived_utils| {
            cloned!(ctx, repo);
            async move {
                let (stats, result) = derived_utils
                    .derive(ctx.clone(), repo.clone(), csid)
                    .timed()
                    .await;
                info!(
                    ctx.logger(),
                    "derived {} in {:?}: {:?}",
                    derived_utils.name(),
                    stats.completion_time,
                    result
                );
                Ok(())
            }
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use blobstore::Blobstore;
    use blobstore::BlobstoreBytes;
    use blobstore::BlobstoreGetData;
    use derived_data::BonsaiDerived;
    use derived_data_manager::BonsaiDerivable;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use mercurial_types::HgChangesetId;
    use std::str::FromStr;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use tests_utils::resolve_cs_id;
    use unodes::RootUnodeManifestId;

    #[fbinit::test]
    async fn test_tail_one_iteration(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Linear::getrepo(fb).await;

        let mut bookmarks_subscription = repo
            .bookmarks()
            .create_subscription(&ctx, Freshness::MostRecent)
            .await
            .context("Error creating bookmarks subscription")?;

        let derived_utils = derived_data_utils(fb, &repo, RootUnodeManifestId::NAME)?;
        let master = resolve_cs_id(&ctx, &repo, "master").await?;
        assert!(!RootUnodeManifestId::is_derived(&ctx, &repo, &master).await?);
        tail_one_iteration(&ctx, &repo, &[derived_utils], &mut bookmarks_subscription).await?;
        assert!(RootUnodeManifestId::is_derived(&ctx, &repo, &master).await?);

        Ok(())
    }

    #[fbinit::test]
    async fn test_single(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let mut repo = Linear::get_inner_repo(fb).await;

        let mut counting_blobstore = None;
        repo.blob_repo = repo
            .blob_repo
            .dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
                let blobstore = Arc::new(CountingBlobstore::new(blobstore));
                counting_blobstore = Some(blobstore.clone());
                blobstore
            });
        let counting_blobstore = counting_blobstore.unwrap();

        let master = resolve_cs_id(&ctx, &repo.blob_repo, "master").await?;
        subcommand_single(
            &ctx,
            &repo.blob_repo,
            master,
            vec![RootUnodeManifestId::NAME.to_string()],
        )
        .await?;

        let writes_count = counting_blobstore.writes_count();
        subcommand_single(
            &ctx,
            &repo.blob_repo,
            master,
            vec![RootUnodeManifestId::NAME.to_string()],
        )
        .await?;
        assert!(counting_blobstore.writes_count() > writes_count);
        Ok(())
    }

    #[fbinit::test]
    async fn test_backfill_data_latest(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Linear::getrepo(fb).await;

        let hg_cs_id = HgChangesetId::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb")?;
        let bcs_id = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, hg_cs_id)
            .await?
            .unwrap();

        let derived_utils = derived_data_utils(fb, &repo, RootUnodeManifestId::NAME)?;
        // The dependencies haven't been derived yet, so this should be an
        // error.
        assert!(
            derived_utils
                .backfill_batch_dangerous(ctx.clone(), repo.clone(), vec![bcs_id], false, None)
                .await
                .is_err()
        );

        let parent_hg_cs_id = HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a")?;
        let parent_bcs_id = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, parent_hg_cs_id)
            .await?
            .unwrap();
        derived_utils
            .derive(ctx.clone(), repo.clone(), parent_bcs_id)
            .await?;

        // Now the parent is derived, we can backfill a batch.
        derived_utils
            .backfill_batch_dangerous(ctx.clone(), repo, vec![bcs_id], false, None)
            .await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_backfill_data_batch(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo = Linear::getrepo(fb).await;

        let mut batch = vec![];
        let hg_cs_ids = vec![
            "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536",
            "3e0e761030db6e479a7fb58b12881883f9f8c63f",
            "607314ef579bd2407752361ba1b0c1729d08b281",
            "d0a361e9022d226ae52f689667bd7d212a19cfe0",
            "cb15ca4a43a59acff5388cea9648c162afde8372",
            "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b",
            "0ed509bf086fadcb8a8a5384dc3b550729b0fc17",
            "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
            "3c15267ebf11807f3d772eb891272b911ec68759",
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            "79a13814c5ce7330173ec04d279bf95ab3f652fb",
        ];
        for hg_cs_id in &hg_cs_ids {
            let hg_cs_id = HgChangesetId::from_str(hg_cs_id)?;
            let maybe_bcs_id = repo
                .bonsai_hg_mapping()
                .get_bonsai_from_hg(&ctx, hg_cs_id)
                .await?;
            batch.push(maybe_bcs_id.unwrap());
        }

        let derived_utils = derived_data_utils(fb, &repo, RootUnodeManifestId::NAME)?;
        let pending = derived_utils
            .pending(ctx.clone(), repo.clone(), batch.clone())
            .await?;
        assert_eq!(pending.len(), hg_cs_ids.len());
        derived_utils
            .backfill_batch_dangerous(ctx.clone(), repo.clone(), batch.clone(), false, None)
            .await?;
        let pending = derived_utils.pending(ctx, repo, batch).await?;
        assert_eq!(pending.len(), 0);

        Ok(())
    }

    #[fbinit::test]
    async fn test_backfill_data_failing_blobstore(fb: FacebookInit) -> Result<()> {
        // The test exercises that derived data mapping entries are written only after
        // all other blobstore writes were successful i.e. mapping entry shouldn't exist
        // if any of the corresponding blobs weren't successfully saved
        let ctx = CoreContext::test_mock(fb);
        let origrepo = Linear::getrepo(fb).await;

        let repo = origrepo.dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
            Arc::new(FailingBlobstore::new("manifest".to_string(), blobstore))
        });

        let first_hg_cs_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let first_bcs_id = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, first_hg_cs_id)
            .await?
            .unwrap();

        let derived_utils = derived_data_utils(fb, &repo, RootUnodeManifestId::NAME)?;
        let res = derived_utils
            .backfill_batch_dangerous(ctx.clone(), repo.clone(), vec![first_bcs_id], false, None)
            .await;
        // Deriving should fail because blobstore writes fail
        assert!(res.is_err());

        // Make sure that since deriving for first_hg_cs_id failed it didn't
        // write any mapping entries. And because it didn't deriving the
        // next changeset is still safe
        let repo = origrepo;
        let second_hg_cs_id = HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f")?;
        let second_bcs_id = repo
            .bonsai_hg_mapping()
            .get_bonsai_from_hg(&ctx, second_hg_cs_id)
            .await?
            .unwrap();
        let batch = vec![first_bcs_id, second_bcs_id];
        let derived_utils = derived_data_utils(fb, &repo, RootUnodeManifestId::NAME)?;
        assert_eq!(
            derived_utils
                .pending(ctx.clone(), repo.clone(), batch.clone())
                .await?,
            batch,
        );
        derived_utils
            .backfill_batch_dangerous(ctx, repo, batch, false, None)
            .await?;

        Ok(())
    }

    #[derive(Debug)]
    struct FailingBlobstore {
        bad_key_substring: String,
        inner: Arc<dyn Blobstore>,
    }

    impl FailingBlobstore {
        fn new(bad_key_substring: String, inner: Arc<dyn Blobstore>) -> Self {
            Self {
                bad_key_substring,
                inner,
            }
        }
    }

    impl std::fmt::Display for FailingBlobstore {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "FailingBlobstore")
        }
    }

    #[async_trait]
    impl Blobstore for FailingBlobstore {
        async fn put<'a>(
            &'a self,
            ctx: &'a CoreContext,
            key: String,
            value: BlobstoreBytes,
        ) -> Result<()> {
            if key.contains(&self.bad_key_substring) {
                tokio::time::sleep(Duration::from_millis(250)).await;
                Err(format_err!("failed"))
            } else {
                self.inner.put(ctx, key, value).await
            }
        }

        async fn get<'a>(
            &'a self,
            ctx: &'a CoreContext,
            key: &'a str,
        ) -> Result<Option<BlobstoreGetData>> {
            self.inner.get(ctx, key).await
        }
    }

    #[derive(Debug)]
    struct CountingBlobstore {
        count: AtomicUsize,
        inner: Arc<dyn Blobstore>,
    }

    impl std::fmt::Display for CountingBlobstore {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "CountingBlobstore")
        }
    }

    impl CountingBlobstore {
        fn new(inner: Arc<dyn Blobstore>) -> Self {
            Self {
                count: AtomicUsize::new(0),
                inner,
            }
        }

        fn writes_count(&self) -> usize {
            self.count.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl Blobstore for CountingBlobstore {
        async fn put<'a>(
            &'a self,
            ctx: &'a CoreContext,
            key: String,
            value: BlobstoreBytes,
        ) -> Result<()> {
            self.count.fetch_add(1, Ordering::Relaxed);
            self.inner.put(ctx, key, value).await
        }

        async fn get<'a>(
            &'a self,
            ctx: &'a CoreContext,
            key: &'a str,
        ) -> Result<Option<BlobstoreGetData>> {
            self.inner.get(ctx, key).await
        }
    }
}
