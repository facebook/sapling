/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "15000000"]
#![deny(warnings)]
#![feature(map_first_last)]

use anyhow::{anyhow, format_err, Error, Result};
use blame::BlameRoot;
use blobrepo::BlobRepo;
use blobrepo_override::DangerousOverride;
use bookmarks::{BookmarkKind, BookmarkPagination, BookmarkPrefix, Freshness};
use bulkops::fetch_all_public_changesets;
use bytes::Bytes;
use cacheblob::{dummy::DummyLease, InProcessLease, LeaseOps};
use changesets::{deserialize_cs_entries, serialize_cs_entries, ChangesetEntry, SqlChangesets};
use clap::{Arg, SubCommand};
use cloned::cloned;
use cmdlib::{
    args::{self, MononokeMatches},
    helpers,
};
use context::CoreContext;
use derived_data::BonsaiDerivable;
use derived_data_utils::{
    derived_data_utils, derived_data_utils_for_backfill, DerivedUtils, ThinOut,
    POSSIBLE_DERIVED_TYPES,
};
use fbinit::FacebookInit;
use fsnodes::RootFsnodeId;
use futures::{
    compat::Future01CompatExt,
    future::{self, try_join},
    stream::{self, StreamExt, TryStreamExt},
};
use futures_stats::TimedFutureExt;
use metaconfig_types::RepoConfig;
use mononoke_types::{ChangesetId, DateTime};
use skiplist::{fetch_skiplist_index, SkiplistIndex};
use slog::{info, Logger};
use stats::prelude::*;
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use time_ext::DurationExt;

mod dry_run;
mod slice;
mod warmup;

define_stats! {
    prefix = "mononoke.derived_data";
    oldest_underived_secs: dynamic_singleton_counter("{}.oldest_underived_secs", (reponame: String)),
    derivation_time_ms: dynamic_timeseries("{}.derivation_time_ms", (reponame: String); Average, Sum),
}

const ARG_ALL_TYPES: &str = "all-types";
const ARG_DERIVED_DATA_TYPE: &str = "derived-data-type";
const ARG_DRY_RUN: &str = "dry-run";
const ARG_OUT_FILENAME: &str = "out-filename";
const ARG_SKIP: &str = "skip-changesets";
const ARG_LIMIT: &str = "limit";
const ARG_REGENERATE: &str = "regenerate";
const ARG_PREFETCHED_COMMITS_PATH: &str = "prefetched-commits-path";
const ARG_CHANGESET: &str = "changeset";
const ARG_USE_SHARED_LEASES: &str = "use-shared-leases";
const ARG_BATCHED: &str = "batched";
const ARG_BATCH_SIZE: &str = "batch-size";
const ARG_PARALLEL: &str = "parallel";
const ARG_SLICED: &str = "sliced";
const ARG_SLICE_SIZE: &str = "slice-size";
const ARG_BACKFILL: &str = "backfill";

const SUBCOMMAND_BACKFILL: &str = "backfill";
const SUBCOMMAND_BACKFILL_ALL: &str = "backfill-all";
const SUBCOMMAND_TAIL: &str = "tail";
const SUBCOMMAND_PREFETCH_COMMITS: &str = "prefetch-commits";
const SUBCOMMAND_SINGLE: &str = "single";

const DEFAULT_BATCH_SIZE_STR: &str = "4096";
const DEFAULT_SLICE_SIZE_STR: &str = "20000";

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

async fn open_repo_maybe_unredacted(
    fb: FacebookInit,
    logger: &Logger,
    matches: &MononokeMatches<'_>,
    data_type: &str,
) -> Result<BlobRepo> {
    if UNREDACTED_TYPES.contains(&data_type) {
        args::open_repo_unredacted(fb, logger, matches).await
    } else {
        args::open_repo(fb, logger, matches).await
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = args::MononokeAppBuilder::new("Utility to work with bonsai derived data")
        .with_advanced_args_hidden()
        .with_fb303_args()
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
                    Arg::with_name(ARG_DRY_RUN)
                        .long(ARG_DRY_RUN)
                        .takes_value(false)
                        .required(false)
                        .help(
                            "Derives all data but writes it to memory. Note - requires --readonly",
                        ),
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
                ),
        )
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_PREFETCH_COMMITS)
                .about("fetch commits metadata from the database and save them to a file")
                .arg(
                    Arg::with_name(ARG_OUT_FILENAME)
                        .long(ARG_OUT_FILENAME)
                        .takes_value(true)
                        .required(true)
                        .help("file name where commits will be saved"),
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
                        .help("changeset by {hd|bonsai} hash or bookmark"),
                )
                .arg(
                    Arg::with_name(ARG_DERIVED_DATA_TYPE)
                        .required(false)
                        .index(2)
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
                    Arg::with_name(ARG_SLICED).long(ARG_SLICED).help(
                        "pre-slice repository into generation slices using the skiplist index",
                    ),
                )
                .arg(
                    Arg::with_name(ARG_SLICE_SIZE)
                        .long(ARG_SLICE_SIZE)
                        .default_value(DEFAULT_SLICE_SIZE_STR)
                        .help("number of generations to include in each generation slice"),
                ),
        );
    let matches = app.get_matches();
    args::init_cachelib(fb, &matches);

    let logger = args::init_logging(fb, &matches);
    args::init_config_store(fb, &logger, &matches)?;
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    helpers::block_execute(
        run_subcmd(fb, ctx, &logger, &matches),
        fb,
        &std::env::var("TW_JOB_NAME").unwrap_or("backfill_derived_data".to_string()),
        &logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}

async fn run_subcmd<'a>(
    fb: FacebookInit,
    ctx: CoreContext,
    logger: &Logger,
    matches: &'a MononokeMatches<'a>,
) -> Result<()> {
    match matches.subcommand() {
        (SUBCOMMAND_BACKFILL_ALL, Some(sub_m)) => {
            let repo = args::open_repo_unredacted(fb, logger, matches).await?;
            let config_store = args::init_config_store(fb, logger, matches)?;
            let (_, config) = args::get_config_by_repoid(config_store, matches, repo.get_repoid())?;
            let derived_data_types = sub_m.values_of(ARG_DERIVED_DATA_TYPE).map_or_else(
                || {
                    let config = repo.get_derived_data_config();
                    &config.enabled.types | &config.backfilling.types
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

            subcommand_backfill_all(
                &ctx,
                &repo,
                &config,
                derived_data_types,
                slice_size,
                batch_size,
                parallel,
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

            let mut repo =
                open_repo_maybe_unredacted(fb, &logger, &matches, &derived_data_type).await?;

            info!(
                ctx.logger(),
                "reading all changesets for: {:?}",
                repo.get_repoid()
            );
            let mut changesets = parse_serialized_commits(prefetched_commits_path)?;
            changesets.sort_by_key(|cs_entry| cs_entry.gen);

            let mut cleaner = None;

            if sub_m.is_present(ARG_DRY_RUN) {
                if !args::parse_readonly_storage(matches).0 {
                    return Err(anyhow!("--dry-run requires readonly storage!"));
                }

                if derived_data_type != "fsnodes" {
                    return Err(anyhow!("unsupported dry run data type"));
                }

                let mut children_count = HashMap::new();
                for entry in &changesets {
                    for p in &entry.parents {
                        *children_count.entry(*p).or_insert(0) += 1;
                    }
                }

                if derived_data_type == "fsnodes" {
                    let (new_cleaner, wrapped_repo) = dry_run::FsnodeCleaner::new(
                        ctx.clone(),
                        repo.clone(),
                        children_count,
                        10000,
                    );
                    repo = wrapped_repo;
                    cleaner = Some(new_cleaner);
                }
            }

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

            subcommand_backfill(
                &ctx,
                &repo,
                derived_data_type.as_str(),
                regenerate,
                parallel,
                batch_size,
                changesets,
                cleaner,
            )
            .await
        }
        (SUBCOMMAND_TAIL, Some(sub_m)) => {
            let repo = args::open_repo_unredacted(fb, &logger, &matches).await?;
            let config_store = args::init_config_store(fb, logger, matches)?;
            let (_, config) = args::get_config_by_repoid(config_store, matches, repo.get_repoid())?;
            let use_shared_leases = sub_m.is_present(ARG_USE_SHARED_LEASES);
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
            subcommand_tail(
                &ctx,
                repo,
                config,
                use_shared_leases,
                batch_size,
                parallel,
                backfill,
                slice_size,
            )
            .await
        }
        (SUBCOMMAND_PREFETCH_COMMITS, Some(sub_m)) => {
            let config_store = args::init_config_store(fb, logger, &matches)?;
            let out_filename = sub_m
                .value_of(ARG_OUT_FILENAME)
                .ok_or_else(|| format_err!("missing required argument: {}", ARG_OUT_FILENAME))?
                .to_string();

            let (repo, changesets) = try_join(
                args::open_repo(fb, &logger, &matches),
                args::open_sql::<SqlChangesets>(fb, config_store, &matches),
            )
            .await?;
            let phases = repo.get_phases();
            let sql_phases = phases.get_sql_phases();
            let css =
                fetch_all_public_changesets(&ctx, repo.get_repoid(), &changesets, &sql_phases)
                    .try_collect()
                    .await?;

            let serialized = serialize_cs_entries(css);
            Ok(fs::write(out_filename, serialized)?)
        }
        (SUBCOMMAND_SINGLE, Some(sub_m)) => {
            let hash_or_bookmark = sub_m
                .value_of_lossy(ARG_CHANGESET)
                .ok_or_else(|| format_err!("missing required argument: {}", ARG_CHANGESET))?
                .to_string();
            let all = sub_m.is_present(ARG_ALL_TYPES);
            let derived_data_type = sub_m.value_of(ARG_DERIVED_DATA_TYPE);
            let (repo, types): (_, Vec<String>) = match (all, derived_data_type) {
                (true, None) => {
                    let repo = args::open_repo_unredacted(fb, logger, matches).await?;
                    let types = repo
                        .get_derived_data_config()
                        .enabled
                        .types
                        .iter()
                        .cloned()
                        .collect();
                    (repo, types)
                }
                (false, Some(derived_data_type)) => {
                    let repo =
                        open_repo_maybe_unredacted(fb, &logger, &matches, &derived_data_type)
                            .await?;
                    (repo, vec![derived_data_type.to_string()])
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
            let csid = helpers::csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
                .compat()
                .await?;
            subcommand_single(&ctx, &repo, csid, types).await
        }
        (name, _) => Err(format_err!("unhandled subcommand: {}", name)),
    }
}

fn parse_serialized_commits<P: AsRef<Path>>(file: P) -> Result<Vec<ChangesetEntry>> {
    let data = fs::read(file)?;
    deserialize_cs_entries(&Bytes::from(data))
}

async fn subcommand_backfill_all(
    ctx: &CoreContext,
    repo: &BlobRepo,
    config: &RepoConfig,
    derived_data_types: HashSet<String>,
    slice_size: Option<u64>,
    batch_size: usize,
    parallel: bool,
) -> Result<()> {
    info!(ctx.logger(), "derived data types: {:?}", derived_data_types);
    let derivers = derived_data_types
        .iter()
        .map(|name| derived_data_utils_for_backfill(repo, name.as_str()))
        .collect::<Result<Vec<_>, _>>()?;
    let skiplist_index = fetch_skiplist_index(
        ctx,
        &config.skiplist_index_blobstore_key,
        &repo.blobstore().boxed(),
    )
    .await?;

    let heads = get_most_recent_heads(ctx, repo).await?;
    backfill_heads(
        ctx,
        repo,
        Some(&skiplist_index),
        derivers.as_ref(),
        heads,
        slice_size,
        batch_size,
        parallel,
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
            tail_batch_iteration(ctx, repo, derivers, slice_heads, batch_size, parallel).await?;
        }
    } else {
        info!(ctx.logger(), "Deriving {} heads", heads.len());
        tail_batch_iteration(ctx, repo, derivers, heads, batch_size, parallel).await?;
    }
    Ok(())
}

fn truncate_duration(duration: Duration) -> Duration {
    Duration::from_secs(duration.as_secs())
}

async fn subcommand_backfill(
    ctx: &CoreContext,
    repo: &BlobRepo,
    derived_data_type: &str,
    regenerate: bool,
    parallel: bool,
    batch_size: usize,
    changesets: Vec<ChangesetId>,
    mut cleaner: Option<impl dry_run::Cleaner>,
) -> Result<()> {
    let derived_utils = &derived_data_utils_for_backfill(repo, derived_data_type)?;

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
                .pending(ctx.clone(), repo.clone(), chunk.to_vec())
                .await?;
            let chunk_size = chunk.len();

            warmup::warmup(ctx, repo, derived_data_type.as_ref(), &chunk).await?;
            info!(ctx.logger(), "warmup of {} changesets complete", chunk_size);

            derived_utils
                .backfill_batch_dangerous(ctx.clone(), repo.clone(), chunk, parallel)
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
        if let Some(ref mut cleaner) = cleaner {
            cleaner.clean(chunk.to_vec()).await?;
        }
    }
    Ok(())
}

async fn subcommand_tail(
    ctx: &CoreContext,
    repo: BlobRepo,
    config: RepoConfig,
    use_shared_leases: bool,
    batch_size: Option<usize>,
    parallel: bool,
    mut backfill: bool,
    slice_size: Option<u64>,
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

    let derived_data_config = repo.get_derived_data_config();

    let tail_derivers: Vec<Arc<dyn DerivedUtils>> = derived_data_config
        .enabled
        .types
        .iter()
        .map(|name| derived_data_utils(&repo, name))
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
    let backfill_derivers: Vec<Arc<dyn DerivedUtils>> =
        if backfill && !derived_data_config.backfilling.types.is_empty() {
            // Some backfilling types may depend on enabled types for their
            // derivation.  This means we need to include the appropriate
            // derivers for all types (enabled and backfilling).  Since the
            // enabled type will already have been derived, the deriver for
            // those types will just be used for mapping look-ups.  The
            // `derived_data_utils_for_backfill` function takes care of giving
            // us the right deriver type for the config.
            derived_data_config
                .enabled
                .types
                .union(&derived_data_config.backfilling.types)
                .map(|name| derived_data_utils_for_backfill(repo, name))
                .collect::<Result<_>>()?
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
    let skiplist_index = if backfill {
        Some(
            fetch_skiplist_index(
                ctx,
                &config.skiplist_index_blobstore_key,
                &repo.blobstore().boxed(),
            )
            .await?,
        )
    } else {
        None
    };

    if let Some(batch_size) = batch_size {
        info!(ctx.logger(), "using batched deriver");

        let (sender, mut receiver) = tokio::sync::watch::channel(HashSet::new());

        let tail_loop = async move {
            cloned!(ctx, repo);
            tokio::spawn(async move {
                let mut derived_heads = HashSet::new();
                loop {
                    let heads = match get_most_recent_heads(&ctx, &repo).await {
                        Ok(heads) => heads.into_iter().collect::<HashSet<_>>(),
                        Err(e) => return Err::<(), _>(e),
                    };
                    tail_batch_iteration(
                        &ctx,
                        &repo,
                        &tail_derivers,
                        heads.difference(&derived_heads).cloned().collect(),
                        batch_size,
                        parallel,
                    )
                    .await?;
                    let _ = sender.broadcast(heads.clone());
                    derived_heads = heads;
                }
            })
            .await?
        };

        let backfill_loop = async move {
            cloned!(ctx, repo);
            tokio::spawn(async move {
                if backfill {
                    let mut derived_heads = HashSet::new();
                    while let Some(heads) = receiver.recv().await {
                        backfill_heads(
                            &ctx,
                            &repo,
                            skiplist_index.as_deref(),
                            &backfill_derivers,
                            heads.difference(&derived_heads).cloned().collect(),
                            slice_size,
                            batch_size,
                            parallel,
                        )
                        .await?;
                        derived_heads = heads;
                    }
                }
                Ok::<_, Error>(())
            })
            .await?
        };

        try_join(tail_loop, backfill_loop).await?;
    } else {
        info!(ctx.logger(), "using simple deriver");
        loop {
            tail_one_iteration(ctx, repo, &tail_derivers).await?;
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
        tokio::time::delay_for(Duration::from_millis(250)).await;
    } else {
        info!(ctx.logger(), "deriving data {}", size);
        // We are using `bounded_traversal_dag` directly instead of `DeriveGraph::derive`
        // so we could use `warmup::warmup` on each node.
        bounded_traversal::bounded_traversal_dag(
            100,
            derive_graph,
            |node| async move {
                let deps = node.dependencies.clone();
                Ok((node, deps))
            },
            move |node, _| {
                cloned!(ctx, repo);
                async move {
                    if let Some(deriver) = &node.deriver {
                        warmup::warmup(&ctx, &repo, deriver.name(), &node.csids).await?;
                        let timestamp = Instant::now();
                        deriver
                            .backfill_batch_dangerous(
                                ctx.clone(),
                                repo,
                                node.csids.clone(),
                                parallel,
                            )
                            .await?;
                        if let (Some(first), Some(last)) = (node.csids.first(), node.csids.last()) {
                            slog::info!(
                                ctx.logger(),
                                "[{}:{}] count:{} time:{:?} start:{} end:{}",
                                deriver.name(),
                                node.id,
                                node.csids.len(),
                                timestamp.elapsed(),
                                first,
                                last
                            );
                        }
                    }
                    Result::<_>::Ok(())
                }
            },
        )
        .await?
        .ok_or_else(|| anyhow!("derive graph contains a cycle"))?;
    }

    Ok(())
}

async fn tail_one_iteration(
    ctx: &CoreContext,
    repo: &BlobRepo,
    derive_utils: &[Arc<dyn DerivedUtils>],
) -> Result<()> {
    let heads = get_most_recent_heads(ctx, repo).await?;

    // Find heads that needs derivation and find their oldest underived ancestor
    let find_pending_futs: Vec<_> = derive_utils
        .iter()
        .map({
            |derive| {
                let heads = heads.clone();
                async move {
                    // create new context so each derivation would have its own trace
                    let ctx = CoreContext::new_with_logger(ctx.fb, ctx.logger().clone());
                    let pending = derive.pending(ctx.clone(), repo.clone(), heads).await?;

                    let oldest_underived =
                        derive.find_oldest_underived(&ctx, &repo, &pending).await?;
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
            .map(|csid| derive.derive(ctx.clone(), repo.clone(), csid))
            .collect::<Vec<_>>()
    });

    let pending_futs: Vec<_> = pending_futs.flatten().collect();

    if pending_futs.is_empty() {
        tokio::time::delay_for(Duration::from_millis(250)).await;
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
        let utils = derived_data_utils(&repo, ty)?;
        utils.regenerate(&vec![csid]);
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
    use blobrepo_hg::BlobRepoHg;
    use blobstore::{Blobstore, BlobstoreBytes, BlobstoreGetData};
    use derived_data::BonsaiDerived;
    use fixtures::linear;
    use mercurial_types::HgChangesetId;
    use std::{
        str::FromStr,
        sync::atomic::{AtomicUsize, Ordering},
    };
    use tests_utils::resolve_cs_id;
    use unodes::RootUnodeManifestId;

    #[fbinit::compat_test]
    async fn test_tail_one_iteration(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo = linear::getrepo(fb).await;
        let derived_utils = derived_data_utils(&repo, RootUnodeManifestId::NAME)?;
        let master = resolve_cs_id(&ctx, &repo, "master").await?;
        assert!(!RootUnodeManifestId::is_derived(&ctx, &repo, &master).await?);
        tail_one_iteration(&ctx, &repo, &[derived_utils]).await?;
        assert!(RootUnodeManifestId::is_derived(&ctx, &repo, &master).await?);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_single(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo = linear::getrepo(fb).await;

        let mut counting_blobstore = None;
        let repo = repo.dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
            let blobstore = Arc::new(CountingBlobstore::new(blobstore));
            counting_blobstore = Some(blobstore.clone());
            blobstore
        });
        let counting_blobstore = counting_blobstore.unwrap();

        let master = resolve_cs_id(&ctx, &repo, "master").await?;
        subcommand_single(
            &ctx,
            &repo,
            master,
            vec![RootUnodeManifestId::NAME.to_string()],
        )
        .await?;

        let writes_count = counting_blobstore.writes_count();
        subcommand_single(
            &ctx,
            &repo,
            master,
            vec![RootUnodeManifestId::NAME.to_string()],
        )
        .await?;
        assert!(counting_blobstore.writes_count() > writes_count);
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_backfill_data_latest(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo = linear::getrepo(fb).await;

        let hg_cs_id = HgChangesetId::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb")?;
        let maybe_bcs_id = repo
            .get_bonsai_from_hg(ctx.clone(), hg_cs_id)
            .compat()
            .await?;
        let bcs_id = maybe_bcs_id.unwrap();

        let derived_utils = derived_data_utils(&repo, RootUnodeManifestId::NAME)?;
        derived_utils
            .backfill_batch_dangerous(ctx, repo, vec![bcs_id], false)
            .await?;

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_backfill_data_batch(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo = linear::getrepo(fb).await;

        let mut batch = vec![];
        let hg_cs_ids = vec![
            "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
            "3c15267ebf11807f3d772eb891272b911ec68759",
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            "79a13814c5ce7330173ec04d279bf95ab3f652fb",
        ];
        for hg_cs_id in &hg_cs_ids {
            let hg_cs_id = HgChangesetId::from_str(hg_cs_id)?;
            let maybe_bcs_id = repo
                .get_bonsai_from_hg(ctx.clone(), hg_cs_id)
                .compat()
                .await?;
            batch.push(maybe_bcs_id.unwrap());
        }

        let derived_utils = derived_data_utils(&repo, RootUnodeManifestId::NAME)?;
        let pending = derived_utils
            .pending(ctx.clone(), repo.clone(), batch.clone())
            .await?;
        assert_eq!(pending.len(), hg_cs_ids.len());
        derived_utils
            .backfill_batch_dangerous(ctx.clone(), repo.clone(), batch.clone(), false)
            .await?;
        let pending = derived_utils.pending(ctx, repo, batch).await?;
        assert_eq!(pending.len(), 0);

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_backfill_data_failing_blobstore(fb: FacebookInit) -> Result<()> {
        // The test exercises that derived data mapping entries are written only after
        // all other blobstore writes were successful i.e. mapping entry shouldn't exist
        // if any of the corresponding blobs weren't successfully saved
        let ctx = CoreContext::test_mock(fb);
        let origrepo = linear::getrepo(fb).await;

        let repo = origrepo.dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
            Arc::new(FailingBlobstore::new("manifest".to_string(), blobstore))
        });

        let first_hg_cs_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let maybe_bcs_id = repo
            .get_bonsai_from_hg(ctx.clone(), first_hg_cs_id)
            .compat()
            .await?;
        let bcs_id = maybe_bcs_id.unwrap();

        let derived_utils = derived_data_utils(&repo, RootUnodeManifestId::NAME)?;
        let res = derived_utils
            .backfill_batch_dangerous(ctx.clone(), repo.clone(), vec![bcs_id], false)
            .await;
        // Deriving should fail because blobstore writes fail
        assert!(res.is_err());

        // Make sure that since deriving for first_hg_cs_id failed it didn't
        // write any mapping entries. And because it didn't deriving the parent changeset
        // is now safe
        let repo = origrepo;
        let second_hg_cs_id = HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f")?;
        let maybe_bcs_id = repo
            .get_bonsai_from_hg(ctx.clone(), second_hg_cs_id)
            .compat()
            .await?;
        let bcs_id = maybe_bcs_id.unwrap();
        derived_utils
            .backfill_batch_dangerous(ctx, repo, vec![bcs_id], false)
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

    #[async_trait]
    impl Blobstore for FailingBlobstore {
        async fn put<'a>(
            &'a self,
            ctx: &'a CoreContext,
            key: String,
            value: BlobstoreBytes,
        ) -> Result<()> {
            if key.find(&self.bad_key_substring).is_some() {
                tokio::time::delay_for(Duration::from_millis(250)).await;
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
