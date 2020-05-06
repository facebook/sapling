/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "15000000"]
#![deny(warnings)]

use anyhow::{anyhow, format_err, Error};
use blame::BlameRoot;
use blobrepo::{BlobRepo, DangerousOverride};
use bookmarks::{BookmarkPrefix, Bookmarks, Freshness};
use bytes::Bytes;
use cacheblob::{dummy::DummyLease, LeaseOps};
use changesets::{
    deserialize_cs_entries, serialize_cs_entries, ChangesetEntry, Changesets, SqlChangesets,
};
use clap::{Arg, ArgMatches, SubCommand};
use cloned::cloned;
use cmdlib::{args, helpers};
use context::CoreContext;
use dbbookmarks::SqlBookmarks;
use derived_data::BonsaiDerived;
use derived_data_utils::{
    derived_data_utils, derived_data_utils_unsafe, DerivedUtils, POSSIBLE_DERIVED_TYPES,
};
use fbinit::FacebookInit;
use fsnodes::RootFsnodeId;
use futures::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{self, try_join, try_join3, TryFutureExt},
    stream::{self, StreamExt, TryStreamExt},
    Stream,
};
use futures_ext::FutureExt as OldFutureExt;
use futures_old::{Future as OldFuture, Stream as OldStream};
use futures_stats::Timed;
use futures_stats::TimedFutureExt;
use lock_ext::LockExt;
use metaconfig_types::DerivedDataConfig;
use mononoke_types::{ChangesetId, RepositoryId};
use phases::SqlPhases;
use slog::{info, Logger};
use std::{
    collections::HashMap,
    fs,
    path::Path,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

mod warmup;

mod dry_run;

const ARG_DERIVED_DATA_TYPE: &str = "derived-data-type";
const ARG_DRY_RUN: &str = "dry-run";
const ARG_OUT_FILENAME: &str = "out-filename";
const ARG_SKIP: &str = "skip-changesets";
const ARG_REGENERATE: &str = "regenerate";
const ARG_PREFETCHED_COMMITS_PATH: &str = "prefetched-commits-path";
const ARG_CHANGESET: &str = "changeset";

const SUBCOMMAND_BACKFILL: &str = "backfill";
const SUBCOMMAND_TAIL: &str = "tail";
const SUBCOMMAND_PREFETCH_COMMITS: &str = "prefetch-commits";
const SUBCOMMAND_SINGLE: &str = "single";

const CHUNK_SIZE: usize = 4096;

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

fn open_repo_maybe_unredacted<'a>(
    fb: FacebookInit,
    logger: &Logger,
    matches: &ArgMatches<'a>,
    data_type: &str,
) -> impl OldFuture<Item = BlobRepo, Error = Error> {
    if UNREDACTED_TYPES.contains(&data_type) {
        args::open_repo_unredacted(fb, logger, matches).left_future()
    } else {
        args::open_repo(fb, logger, matches).right_future()
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeApp::new("Utility to work with bonsai derived data")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .build()
        .version("0.0.0")
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
                ),
        )
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_TAIL)
                .about("tail public commits and fill derived data")
                .arg(
                    Arg::with_name(ARG_DERIVED_DATA_TYPE)
                        .required(true)
                        .multiple(true)
                        .index(1)
                        .possible_values(POSSIBLE_DERIVED_TYPES)
                        // TODO(stash): T66492899 remove unused value
                        .help("Unused, will be deleted soon"),
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
                    Arg::with_name(ARG_DERIVED_DATA_TYPE)
                        .required(true)
                        .index(1)
                        .possible_values(POSSIBLE_DERIVED_TYPES)
                        .help("derived data type for which backfill will be run"),
                )
                .arg(
                    Arg::with_name(ARG_CHANGESET)
                        .required(true)
                        .index(2)
                        .help("changeset by {hd|bonsai} hash or bookmark"),
                ),
        );
    let matches = app.get_matches();
    args::init_cachelib(fb, &matches, None);

    let logger = args::init_logging(fb, &matches);
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
    matches: &'a ArgMatches<'a>,
) -> Result<(), Error> {
    match matches.subcommand() {
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

            let repo = open_repo_maybe_unredacted(fb, &logger, &matches, &derived_data_type)
                .compat()
                .await?;

            // Backfill is used when when a derived data type is not enabled yet, and so
            // any attempt to call BonsaiDerived::derive() fails. However calling
            // BonsaiDerived::derive() might be useful, and so the lines below explicitly
            // enable `derived_data_type` to allow calling BonsaiDerived::derive() if necessary.
            let mut repo = repo.dangerous_override(|mut derived_data_config: DerivedDataConfig| {
                derived_data_config
                    .derived_data_types
                    .insert(derived_data_type.clone());
                derived_data_config
            });
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

            let changesets: Vec<_> = changesets
                .into_iter()
                .skip(skip)
                .map(|entry| entry.cs_id)
                .collect();

            subcommand_backfill(
                &ctx,
                &repo,
                &derived_data_type,
                regenerate,
                changesets,
                cleaner,
            )
            .await
        }
        (SUBCOMMAND_TAIL, Some(_sub_m)) => {
            let (repo, unredacted_repo, bookmarks) = try_join3(
                args::open_repo(fb, &logger, &matches).compat(),
                args::open_repo_unredacted(fb, &logger, &matches).compat(),
                args::open_sql::<SqlBookmarks>(fb, &matches).compat(),
            )
            .await?;
            subcommand_tail(&ctx, &repo, &unredacted_repo, &bookmarks).await
        }
        (SUBCOMMAND_PREFETCH_COMMITS, Some(sub_m)) => {
            let out_filename = sub_m
                .value_of(ARG_OUT_FILENAME)
                .ok_or_else(|| format_err!("missing required argument: {}", ARG_OUT_FILENAME))?
                .to_string();

            let (repo, changesets) = try_join(
                args::open_repo(fb, &logger, &matches).compat(),
                args::open_sql::<SqlChangesets>(fb, &matches).compat(),
            )
            .await?;
            let phases = repo.get_phases();
            let sql_phases = phases.get_sql_phases();
            let css =
                fetch_all_public_changesets(&ctx, repo.get_repoid(), &changesets, &sql_phases)
                    .try_collect()
                    .await?;

            let serialized = serialize_cs_entries(css);
            fs::write(out_filename, serialized).map_err(Error::from)
        }
        (SUBCOMMAND_SINGLE, Some(sub_m)) => {
            let hash_or_bookmark = sub_m
                .value_of_lossy(ARG_CHANGESET)
                .ok_or_else(|| format_err!("missing required argument: {}", ARG_CHANGESET))?
                .to_string();
            let derived_data_type = sub_m
                .value_of(ARG_DERIVED_DATA_TYPE)
                .ok_or_else(|| format_err!("missing required argument: {}", ARG_DERIVED_DATA_TYPE))?
                .to_string();
            let repo = open_repo_maybe_unredacted(fb, &logger, &matches, &derived_data_type)
                .compat()
                .await?;
            let csid = helpers::csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
                .compat()
                .await?;
            subcommand_single(&ctx, &repo, csid, &derived_data_type).await
        }
        (name, _) => Err(format_err!("unhandled subcommand: {}", name)),
    }
}

fn windows(start: u64, stop: u64, step: u64) -> impl Iterator<Item = (u64, u64)> {
    (0..)
        .map(move |index| (start + index * step, start + (index + 1) * step))
        .take_while(move |(low, _high)| *low < stop)
        .map(move |(low, high)| (low, std::cmp::min(stop, high)))
}

// This function is not optimal since it could be made faster by doing more processing
// on XDB side, but for the puprpose of this binary it is good enough
fn fetch_all_public_changesets<'a>(
    ctx: &'a CoreContext,
    repo_id: RepositoryId,
    changesets: &'a SqlChangesets,
    phases: &'a SqlPhases,
) -> impl Stream<Item = Result<ChangesetEntry, Error>> + 'a {
    async move {
        let (start, stop) = changesets
            .get_changesets_ids_bounds(repo_id.clone())
            .compat()
            .await?;

        let start = start.ok_or_else(|| Error::msg("changesets table is empty"))?;
        let stop = stop.ok_or_else(|| Error::msg("changesets table is empty"))? + 1;
        let step = 65536;
        Ok(stream::iter(windows(start, stop, step)).map(Ok))
    }
    .try_flatten_stream()
    .and_then(move |(lower_bound, upper_bound)| async move {
        let ids = changesets
            .get_list_bs_cs_id_in_range_exclusive(repo_id, lower_bound, upper_bound)
            .compat()
            .try_collect()
            .await?;
        let mut entries = changesets
            .get_many(ctx.clone(), repo_id, ids)
            .compat()
            .await?;
        let cs_ids = entries.iter().map(|entry| entry.cs_id).collect::<Vec<_>>();
        let public = phases.get_public_raw(ctx, &cs_ids).await?;
        entries.retain(|entry| public.contains(&entry.cs_id));
        Ok::<_, Error>(stream::iter(entries).map(Ok))
    })
    .try_flatten()
}

fn parse_serialized_commits<P: AsRef<Path>>(file: P) -> Result<Vec<ChangesetEntry>, Error> {
    let data = fs::read(file).map_err(Error::from)?;
    deserialize_cs_entries(&Bytes::from(data))
}

async fn subcommand_backfill(
    ctx: &CoreContext,
    repo: &BlobRepo,
    derived_data_type: &String,
    regenerate: bool,
    changesets: Vec<ChangesetId>,
    mut cleaner: Option<impl dry_run::Cleaner>,
) -> Result<(), Error> {
    let derived_utils = &derived_data_utils_unsafe(repo.clone(), derived_data_type.clone())?;

    info!(
        ctx.logger(),
        "starting deriving data for {} changesets",
        changesets.len()
    );

    let total_count = changesets.len();
    let generated_count = &Arc::new(AtomicUsize::new(0));
    let total_duration = &Arc::new(Mutex::new(Duration::from_secs(0)));

    if regenerate {
        derived_utils.regenerate(&changesets);
    }

    for chunk in changesets.chunks(CHUNK_SIZE) {
        let (stats, chunk_size) = async {
            let chunk = derived_utils
                .pending(ctx.clone(), repo.clone(), chunk.to_vec())
                .compat()
                .await?;
            let chunk_size = chunk.len();

            warmup::warmup(ctx, repo, derived_data_type, &chunk).await?;

            derived_utils
                .derive_batch(ctx.clone(), repo.clone(), chunk)
                .compat()
                .await?;
            Result::<_, Error>::Ok(chunk_size)
        }
        .timed()
        .await;

        let chunk_size = chunk_size?;
        generated_count.fetch_add(chunk_size, Ordering::SeqCst);
        let elapsed = total_duration.with(|total_duration| {
            *total_duration += stats.completion_time;
            *total_duration
        });

        let generated = generated_count.load(Ordering::SeqCst);
        if generated != 0 {
            let generated = generated as f32;
            let total = total_count as f32;
            info!(
                ctx.logger(),
                "{}/{} estimate:{:.2?} speed:{:.2}/s mean_speed:{:.2}/s",
                generated,
                total_count,
                elapsed.mul_f32((total - generated) / generated),
                chunk_size as f32 / stats.completion_time.as_secs() as f32,
                generated / elapsed.as_secs() as f32,
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
    repo: &BlobRepo,
    unredacted_repo: &BlobRepo,
    bookmarks: &SqlBookmarks,
) -> Result<(), Error> {
    let derive_utils: Vec<(Arc<dyn DerivedUtils>, BlobRepo)> = repo
        .get_derived_data_config()
        .derived_data_types
        .clone()
        .into_iter()
        .map(|name| {
            let maybe_unredacted_repo = if UNREDACTED_TYPES.contains(&name.as_ref()) {
                unredacted_repo.clone()
            } else {
                repo.clone()
            };
            let derive = derived_data_utils(repo.clone(), name)?;
            Ok((derive, maybe_unredacted_repo))
        })
        .collect::<Result<_, Error>>()?;

    loop {
        tail_one_iteration(ctx, repo, bookmarks, &derive_utils).await?;
    }
}

async fn tail_one_iteration(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bookmarks: &SqlBookmarks,
    derive_utils: &[(Arc<dyn DerivedUtils>, BlobRepo)],
) -> Result<(), Error> {
    let heads = bookmarks
        .list_publishing_by_prefix(
            ctx.clone(),
            &BookmarkPrefix::empty(),
            repo.get_repoid(),
            Freshness::MostRecent,
        )
        .map(|(_name, csid)| csid)
        .collect()
        .compat()
        .await?;

    let pending_nested_futs: Vec<_> = derive_utils
        .iter()
        .map({
            |(derive, maybe_unredacted_repo)| {
                let heads = heads.clone();
                async move {
                    // create new context so each derivation would have its own trace
                    let ctx = CoreContext::new_with_logger(ctx.fb, ctx.logger().clone());
                    let pending = derive
                        .pending(ctx.clone(), maybe_unredacted_repo.clone(), heads)
                        .compat()
                        .await?;

                    let derived = pending
                        .into_iter()
                        .map(|csid| {
                            derive
                                .derive(ctx.clone(), maybe_unredacted_repo.clone(), csid)
                                .compat()
                        })
                        .collect::<Vec<_>>();

                    let res: Result<_, Error> = Ok(derived);
                    res
                }
            }
        })
        .collect();

    let pending_futs: Vec<_> = future::try_join_all(pending_nested_futs)
        .await?
        .into_iter()
        .flatten()
        .collect();

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
        Ok(())
    }
}

async fn subcommand_single(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csid: ChangesetId,
    derived_data_type: &str,
) -> Result<(), Error> {
    let repo = repo.dangerous_override(|_| Arc::new(DummyLease {}) as Arc<dyn LeaseOps>);
    let derived_utils = derived_data_utils(repo.clone(), derived_data_type)?;
    derived_utils.regenerate(&vec![csid]);
    derived_utils
        .derive(ctx.clone(), repo, csid)
        .timed({
            cloned!(ctx);
            move |stats, result| {
                info!(
                    ctx.logger(),
                    "derived in {:?}: {:?}", stats.completion_time, result
                );
                Ok(())
            }
        })
        .map(|_| ())
        .compat()
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use blobstore::{Blobstore, BlobstoreBytes, BlobstoreGetData};
    use fixtures::linear;
    use futures::future::FutureExt;
    use futures_ext::BoxFuture;
    use mercurial_types::HgChangesetId;
    use std::str::FromStr;
    use tokio_compat::runtime::Runtime;
    use unodes::RootUnodeManifestId;

    #[fbinit::test]
    fn test_backfill_data_latest(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new()?;

        let ctx = CoreContext::test_mock(fb);
        let repo = runtime.block_on_std(linear::getrepo(fb));

        let hg_cs_id = HgChangesetId::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb")?;
        let maybe_bcs_id = runtime.block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_cs_id))?;
        let bcs_id = maybe_bcs_id.unwrap();

        let derived_utils = derived_data_utils(repo.clone(), RootUnodeManifestId::NAME)?;
        runtime.block_on(derived_utils.derive_batch(ctx.clone(), repo.clone(), vec![bcs_id]))?;

        Ok(())
    }

    #[fbinit::test]
    fn test_backfill_data_batch(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = Runtime::new()?;

        let ctx = CoreContext::test_mock(fb);
        let repo = runtime.block_on_std(linear::getrepo(fb));

        let mut batch = vec![];
        let hg_cs_ids = vec![
            "a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157",
            "3c15267ebf11807f3d772eb891272b911ec68759",
            "a5ffa77602a066db7d5cfb9fb5823a0895717c5a",
            "79a13814c5ce7330173ec04d279bf95ab3f652fb",
        ];
        for hg_cs_id in &hg_cs_ids {
            let hg_cs_id = HgChangesetId::from_str(hg_cs_id)?;
            let maybe_bcs_id = runtime.block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_cs_id))?;
            batch.push(maybe_bcs_id.unwrap());
        }

        let derived_utils = derived_data_utils(repo.clone(), RootUnodeManifestId::NAME)?;
        let pending =
            runtime.block_on(derived_utils.pending(ctx.clone(), repo.clone(), batch.clone()))?;
        assert_eq!(pending.len(), hg_cs_ids.len());
        runtime.block_on(derived_utils.derive_batch(ctx.clone(), repo.clone(), batch.clone()))?;
        let pending = runtime.block_on(derived_utils.pending(ctx.clone(), repo, batch))?;
        assert_eq!(pending.len(), 0);

        Ok(())
    }

    #[fbinit::test]
    fn test_backfill_data_failing_blobstore(fb: FacebookInit) -> Result<(), Error> {
        // The test exercises that derived data mapping entries are written only after
        // all other blobstore writes were successful i.e. mapping entry shouldn't exist
        // if any of the corresponding blobs weren't successfully saved
        let mut runtime = Runtime::new()?;

        let ctx = CoreContext::test_mock(fb);
        let origrepo = runtime.block_on_std(linear::getrepo(fb));

        let repo = origrepo.dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
            Arc::new(FailingBlobstore::new("manifest".to_string(), blobstore))
        });

        let first_hg_cs_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let maybe_bcs_id =
            runtime.block_on(repo.get_bonsai_from_hg(ctx.clone(), first_hg_cs_id))?;
        let bcs_id = maybe_bcs_id.unwrap();

        let derived_utils = derived_data_utils(repo.clone(), RootUnodeManifestId::NAME)?;
        let res =
            runtime.block_on(derived_utils.derive_batch(ctx.clone(), repo.clone(), vec![bcs_id]));
        // Deriving should fail because blobstore writes fail
        assert!(res.is_err());

        // Make sure that since deriving for first_hg_cs_id failed it didn't
        // write any mapping entries. And because it didn't deriving the parent changeset
        // is now safe
        let repo = origrepo;
        let second_hg_cs_id = HgChangesetId::from_str("3e0e761030db6e479a7fb58b12881883f9f8c63f")?;
        let maybe_bcs_id =
            runtime.block_on(repo.get_bonsai_from_hg(ctx.clone(), second_hg_cs_id))?;
        let bcs_id = maybe_bcs_id.unwrap();
        runtime.block_on(derived_utils.derive_batch(ctx.clone(), repo.clone(), vec![bcs_id]))?;

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

    impl Blobstore for FailingBlobstore {
        fn put(
            &self,
            ctx: CoreContext,
            key: String,
            value: BlobstoreBytes,
        ) -> BoxFuture<(), Error> {
            if key.find(&self.bad_key_substring).is_some() {
                tokio::time::delay_for(Duration::from_millis(250))
                    .map(|()| Err(format_err!("failed")))
                    .compat()
                    .boxify()
            } else {
                self.inner.put(ctx, key, value).boxify()
            }
        }

        fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreGetData>, Error> {
            self.inner.get(ctx, key)
        }
    }
}
