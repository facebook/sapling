/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use anyhow::{format_err, Error};
use blame::{fetch_file_full_content, BlameRoot, BlameRootMapping};
use blobrepo::{BlobRepo, DangerousOverride};
use blobstore::{Blobstore, Loadable};
use bookmarks::{BookmarkPrefix, Bookmarks, Freshness};
use bytes::Bytes;
use cacheblob::{dummy::DummyLease, LeaseOps, MemWritesBlobstore};
use changesets::{
    deserialize_cs_entries, serialize_cs_entries, ChangesetEntry, Changesets, SqlChangesets,
};
use clap::{Arg, ArgMatches, SubCommand};
use cloned::cloned;
use cmdlib::{args, helpers, monitoring::start_fb303_and_stats_agg};
use context::CoreContext;
use dbbookmarks::SqlBookmarks;
use deleted_files_manifest::{RootDeletedManifestId, RootDeletedManifestMapping};
use derived_data::{BonsaiDerived, BonsaiDerivedMapping, RegenerateMapping};
use fastlog::{fetch_parent_root_unodes, RootFastlog, RootFastlogMapping};
use fbinit::FacebookInit;
use fsnodes::{RootFsnodeId, RootFsnodeMapping};
use futures::{future, stream, Future, IntoFuture, Stream};
use futures_ext::{spawn_future, try_boxfuture, BoxFuture, FutureExt};
use futures_preview::{compat::Future01CompatExt, future::try_join3, stream::FuturesUnordered};
use futures_stats::Timed;
use futures_util::{
    future::{ready, FutureExt as _, TryFutureExt},
    stream::TryStreamExt,
    try_join,
};
use lock_ext::LockExt;
use manifest::find_intersection_of_diffs;
use mercurial_derived_data::{HgChangesetIdMapping, MappedHgChangesetId};
use mononoke_types::{ChangesetId, FileUnodeId, MPath, RepositoryId};
use phases::SqlPhases;
use slog::{info, Logger};
use stats::prelude::*;
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
use unodes::{find_unode_renames, RootUnodeManifestId, RootUnodeManifestMapping};

define_stats_struct! {
    DerivedDataStats("mononoke.backfill_derived_data.{}.{}", repo_name: String, data_type: &'static str),
    pending_heads: timeseries(Rate, Sum),
}

const ARG_DERIVED_DATA_TYPE: &'static str = "derived-data-type";
const ARG_OUT_FILENAME: &'static str = "out-filename";
const ARG_SKIP: &'static str = "skip-changesets";
const ARG_REGENERATE: &'static str = "regenerate";
const ARG_PREFETCHED_COMMITS_PATH: &'static str = "prefetched-commits-path";
const ARG_CHANGESET: &'static str = "changeset";

const SUBCOMMAND_BACKFILL: &'static str = "backfill";
const SUBCOMMAND_TAIL: &'static str = "tail";
const SUBCOMMAND_PREFETCH_COMMITS: &'static str = "prefetch-commits";
const SUBCOMMAND_SINGLE: &'static str = "single";

const CHUNK_SIZE: usize = 4096;

const POSSIBLE_TYPES: &[&str] = &[
    RootUnodeManifestId::NAME,
    RootFastlog::NAME,
    MappedHgChangesetId::NAME,
    RootFsnodeId::NAME,
    BlameRoot::NAME,
    RootDeletedManifestId::NAME,
];

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

/// Types of derived data for which prefetching content for changed files
/// migth speed up derivation.
const PREFETCH_CONTENT_TYPES: &[&str] = &[BlameRoot::NAME];
const PREFETCH_UNODE_TYPES: &[&str] = &[RootFastlog::NAME, RootDeletedManifestId::NAME];

fn open_repo_maybe_unredacted<'a>(
    fb: FacebookInit,
    logger: &Logger,
    matches: &ArgMatches<'a>,
    data_type: &str,
) -> impl Future<Item = BlobRepo, Error = Error> {
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
                        .possible_values(POSSIBLE_TYPES)
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
                        .possible_values(POSSIBLE_TYPES)
                        .help("comma separated list of derived data types"),
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
                        .possible_values(POSSIBLE_TYPES)
                        .help("derived data type for which backfill will be run"),
                )
                .arg(
                    Arg::with_name(ARG_CHANGESET)
                        .required(true)
                        .index(2)
                        .help("changeset by {hd|bonsai} hash or bookmark"),
                ),
        );
    let app = args::add_fb303_args(app);
    let matches = app.get_matches();
    args::init_cachelib(fb, &matches);

    let logger = args::init_logging(fb, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let mut runtime = args::init_runtime(&matches)?;

    let run = match matches.subcommand() {
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

            let skip = sub_m
                .value_of(ARG_SKIP)
                .map(|skip| skip.parse::<usize>())
                .transpose()
                .map(|skip| skip.unwrap_or(0))
                .into_future()
                .from_err();
            let regenerate = sub_m.is_present(ARG_REGENERATE);

            (
                open_repo_maybe_unredacted(fb, &logger, &matches, &derived_data_type),
                skip,
            )
                .into_future()
                .and_then(move |(repo, skip)| {
                    subcommand_backfill(
                        ctx,
                        repo,
                        derived_data_type,
                        skip,
                        regenerate,
                        prefetched_commits_path,
                    )
                })
                .boxify()
        }
        (SUBCOMMAND_TAIL, Some(sub_m)) => {
            let derived_data_types: Vec<_> = sub_m
                .values_of_lossy(ARG_DERIVED_DATA_TYPE)
                .ok_or_else(|| {
                    format_err!("missing required argument: {}", ARG_DERIVED_DATA_TYPE)
                })?;
            let service_name =
                std::env::var("TW_JOB_NAME").unwrap_or("backfill_derived_data".to_string());

            let stats = {
                let repo_name = format!(
                    "{}_{}",
                    args::get_repo_name(fb, &matches)?,
                    args::get_repo_id(fb, &matches)?
                );
                move |data_type| DerivedDataStats::new(repo_name.clone(), data_type)
            };
            start_fb303_and_stats_agg(fb, &mut runtime, &service_name, &logger, &matches)?;
            (
                args::open_repo(fb, &logger, &matches),
                args::open_repo_unredacted(fb, &logger, &matches),
                args::open_sql::<SqlBookmarks>(fb, &matches),
            )
                .into_future()
                .and_then(move |(repo, unredacted_repo, bookmarks)| {
                    subcommand_tail(
                        ctx,
                        stats,
                        repo,
                        unredacted_repo,
                        bookmarks,
                        derived_data_types,
                    )
                })
                .boxify()
        }
        (SUBCOMMAND_PREFETCH_COMMITS, Some(sub_m)) => {
            let out_filename = sub_m
                .value_of(ARG_OUT_FILENAME)
                .ok_or_else(|| format_err!("missing required argument: {}", ARG_OUT_FILENAME))?
                .to_string();

            (
                args::open_repo(fb, &logger, &matches),
                args::open_sql::<SqlChangesets>(fb, &matches),
                args::open_sql::<SqlPhases>(fb, &matches),
            )
                .into_future()
                .and_then(move |(repo, changesets, phases)| {
                    fetch_all_public_changesets(ctx.clone(), repo.get_repoid(), changesets, phases)
                        .collect()
                })
                .and_then(move |css| {
                    let serialized = serialize_cs_entries(css);
                    fs::write(out_filename, serialized).map_err(Error::from)
                })
                .boxify()
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
            open_repo_maybe_unredacted(fb, &logger, &matches, &derived_data_type)
                .and_then(move |repo| {
                    helpers::csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
                        .and_then(move |csid| subcommand_single(ctx, repo, csid, derived_data_type))
                })
                .boxify()
        }
        (name, _) => {
            return Err(format_err!("unhandled subcommand: {}", name));
        }
    };
    runtime.block_on(run)
}

trait DerivedUtils: Send + Sync + 'static {
    /// Derive data for changeset
    fn derive(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csid: ChangesetId,
    ) -> BoxFuture<String, Error>;

    fn derive_batch(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<(), Error>;

    /// Find pending changeset (changesets for which data have not been derived)
    fn pending(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetId>, Error>;

    /// Regenerate derived data for specified set of commits
    fn regenerate(&self, csids: &Vec<ChangesetId>);

    /// Get a name for this type of derived data
    fn name(&self) -> &'static str;
}

#[derive(Clone)]
struct DerivedUtilsFromMapping<M> {
    mapping: RegenerateMapping<M>,
}

impl<M> DerivedUtilsFromMapping<M> {
    fn new(mapping: M) -> Self {
        let mapping = RegenerateMapping::new(mapping);
        Self { mapping }
    }
}

impl<M> DerivedUtils for DerivedUtilsFromMapping<M>
where
    M: BonsaiDerivedMapping + Clone + 'static,
    M::Value: BonsaiDerived + std::fmt::Debug,
{
    fn derive(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csid: ChangesetId,
    ) -> BoxFuture<String, Error> {
        <M::Value as BonsaiDerived>::derive(ctx.clone(), repo, self.mapping.clone(), csid)
            .map(|result| format!("{:?}", result))
            .boxify()
    }

    fn derive_batch(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<(), Error> {
        let orig_mapping = self.mapping.clone();
        // With InMemoryMapping we can ensure that mapping entries are written only after
        // all corresponding blobs were successfully saved
        let in_memory_mapping = InMemoryMapping::new(self.mapping.clone());

        // Use `MemWritesBlobstore` to avoid blocking on writes to underlying blobstore.
        // `::persist` is later used to bulk write all pending data.
        let mut memblobstore = None;
        let repo = repo
            .dangerous_override(|_| Arc::new(DummyLease {}) as Arc<dyn LeaseOps>)
            .dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
                let blobstore = Arc::new(MemWritesBlobstore::new(blobstore));
                memblobstore = Some(blobstore.clone());
                blobstore
            });
        let memblobstore = memblobstore.expect("memblobstore should have been updated");

        stream::iter_ok(csids)
            .for_each({
                cloned!(ctx, in_memory_mapping, repo);
                move |csid| {
                    // create new context so each derivation would have its own trace
                    let ctx = CoreContext::new_with_logger(ctx.fb, ctx.logger().clone());

                    <M::Value as BonsaiDerived>::derive(
                        ctx.clone(),
                        repo.clone(),
                        in_memory_mapping.clone(),
                        csid,
                    )
                    .map(|_| ())
                }
            })
            .and_then({
                cloned!(ctx, memblobstore);
                move |_| memblobstore.persist(ctx)
            })
            .and_then(move |_| {
                let buffer = in_memory_mapping.into_buffer();
                let buffer = buffer.lock().unwrap();
                let mut futs = vec![];
                for (cs_id, value) in buffer.iter() {
                    futs.push(orig_mapping.put(ctx.clone(), *cs_id, value.clone()));
                }
                stream::futures_unordered(futs).for_each(|_| Ok(()))
            })
            .boxify()
    }

    fn pending(
        &self,
        ctx: CoreContext,
        _repo: BlobRepo,
        mut csids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetId>, Error> {
        self.mapping
            .get(ctx, csids.clone())
            .map(move |derived| {
                csids.retain(|csid| !derived.contains_key(&csid));
                csids
            })
            .boxify()
    }

    fn regenerate(&self, csids: &Vec<ChangesetId>) {
        self.mapping.regenerate(csids.iter().copied())
    }

    fn name(&self) -> &'static str {
        M::Value::NAME
    }
}

#[derive(Clone)]
struct InMemoryMapping<M: BonsaiDerivedMapping + Clone> {
    mapping: M,
    buffer: Arc<Mutex<HashMap<ChangesetId, M::Value>>>,
}

impl<M> InMemoryMapping<M>
where
    M: BonsaiDerivedMapping + Clone,
    <M as BonsaiDerivedMapping>::Value: Clone,
{
    fn new(mapping: M) -> Self {
        Self {
            mapping,
            buffer: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn into_buffer(self) -> Arc<Mutex<HashMap<ChangesetId, M::Value>>> {
        self.buffer
    }
}

impl<M> BonsaiDerivedMapping for InMemoryMapping<M>
where
    M: BonsaiDerivedMapping + Clone,
    <M as BonsaiDerivedMapping>::Value: Clone,
{
    type Value = M::Value;

    fn get(
        &self,
        ctx: CoreContext,
        mut csids: Vec<ChangesetId>,
    ) -> BoxFuture<HashMap<ChangesetId, Self::Value>, Error> {
        let buffer = self.buffer.lock().unwrap();
        let mut ans = HashMap::new();
        csids.retain(|cs_id| {
            if let Some(v) = buffer.get(cs_id) {
                ans.insert(*cs_id, v.clone());
                false
            } else {
                true
            }
        });

        self.mapping
            .get(ctx, csids)
            .map(move |fetched| ans.into_iter().chain(fetched.into_iter()).collect())
            .boxify()
    }

    fn put(&self, _ctx: CoreContext, csid: ChangesetId, id: Self::Value) -> BoxFuture<(), Error> {
        let mut buffer = self.buffer.lock().unwrap();
        buffer.insert(csid, id);
        future::ok(()).boxify()
    }
}

fn derived_data_utils(
    _ctx: CoreContext,
    repo: BlobRepo,
    name: impl AsRef<str>,
) -> Result<Arc<dyn DerivedUtils>, Error> {
    match name.as_ref() {
        RootUnodeManifestId::NAME => {
            let mapping = RootUnodeManifestMapping::new(repo.get_blobstore());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping)))
        }
        RootFastlog::NAME => {
            let mapping = RootFastlogMapping::new(repo.get_blobstore().boxed());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping)))
        }
        MappedHgChangesetId::NAME => {
            let mapping = HgChangesetIdMapping::new(&repo);
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping)))
        }
        RootFsnodeId::NAME => {
            let mapping = RootFsnodeMapping::new(repo.get_blobstore());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping)))
        }
        BlameRoot::NAME => {
            let mapping = BlameRootMapping::new(repo.get_blobstore().boxed());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping)))
        }
        RootDeletedManifestId::NAME => {
            let mapping = RootDeletedManifestMapping::new(repo.get_blobstore());
            Ok(Arc::new(DerivedUtilsFromMapping::new(mapping)))
        }
        name => Err(format_err!("Unsupported derived data type: {}", name)),
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
fn fetch_all_public_changesets(
    ctx: CoreContext,
    repo_id: RepositoryId,
    changesets: SqlChangesets,
    phases: SqlPhases,
) -> impl Stream<Item = ChangesetEntry, Error = Error> {
    changesets
        .get_changesets_ids_bounds(repo_id.clone())
        .and_then(move |(start, stop)| {
            let start = start.ok_or_else(|| Error::msg("changesets table is empty"))?;
            let stop = stop.ok_or_else(|| Error::msg("changesets table is empty"))?;
            let step = 65536;
            Ok(stream::iter_ok(windows(start, stop, step)))
        })
        .flatten_stream()
        .and_then(move |(lower_bound, upper_bound)| {
            changesets
                .get_list_bs_cs_id_in_range(repo_id, lower_bound, upper_bound)
                .collect()
                .and_then({
                    cloned!(ctx, changesets, phases);
                    move |ids| {
                        changesets
                            .get_many(ctx, repo_id, ids)
                            .and_then(move |mut entries| {
                                phases
                                    .get_public_raw(
                                        repo_id,
                                        &entries.iter().map(|entry| entry.cs_id).collect(),
                                    )
                                    .map(move |public| {
                                        entries.retain(|entry| public.contains(&entry.cs_id));
                                        stream::iter_ok(entries)
                                    })
                            })
                    }
                })
        })
        .flatten()
}

fn parse_serialized_commits<P: AsRef<Path>>(file: P) -> Result<Vec<ChangesetEntry>, Error> {
    let data = fs::read(file).map_err(Error::from)?;
    deserialize_cs_entries(&Bytes::from(data))
}

fn subcommand_backfill<P: AsRef<Path>>(
    ctx: CoreContext,
    repo: BlobRepo,
    derived_data_type: String,
    skip: usize,
    regenerate: bool,
    prefetched_commits_path: P,
) -> BoxFuture<(), Error> {
    let derived_utils = try_boxfuture!(derived_data_utils(
        ctx.clone(),
        repo.clone(),
        derived_data_type.clone(),
    ));

    info!(
        ctx.logger(),
        "reading all changesets for: {:?}",
        repo.get_repoid()
    );
    parse_serialized_commits(prefetched_commits_path)
        .into_future()
        .and_then(move |mut changesets| {
            changesets.sort_by_key(|cs_entry| cs_entry.gen);
            let changesets: Vec<_> = changesets
                .into_iter()
                .skip(skip)
                .map(|entry| entry.cs_id)
                .collect();
            info!(
                ctx.logger(),
                "starting deriving data for {} changesets",
                changesets.len()
            );

            let total_count = changesets.len();
            let generated_count = Arc::new(AtomicUsize::new(0));
            let total_duration = Arc::new(Mutex::new(Duration::from_secs(0)));

            if regenerate {
                derived_utils.regenerate(&changesets);
            }

            stream::iter_ok(changesets)
                .chunks(CHUNK_SIZE)
                .and_then({
                    cloned!(ctx, repo, derived_utils);
                    move |chunk| derived_utils.pending(ctx.clone(), repo.clone(), chunk.clone())
                })
                .and_then({
                    cloned!(ctx, derived_data_type, repo);
                    move |chunk| {
                        warmup(
                            ctx.clone(),
                            repo.clone(),
                            derived_data_type.clone(),
                            chunk.clone(),
                        )
                        .boxed()
                        .compat()
                        .map(move |()| chunk)
                    }
                })
                .for_each(move |chunk| {
                    let chunk_size = chunk.len();
                    derived_utils
                        .derive_batch(ctx.clone(), repo.clone(), chunk)
                        .timed({
                            cloned!(ctx, generated_count, total_duration);
                            move |stats, _| {
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
                                Ok(())
                            }
                        })
                })
        })
        .boxify()
}

async fn warmup(
    ctx: CoreContext,
    repo: BlobRepo,
    derived_data_type: String,
    chunk: Vec<ChangesetId>,
) -> Result<(), Error> {
    // Warmup bonsai changesets unconditionally because
    // most likely all derived data needs it. And they are cheap to warm up anyway

    let bcs_warmup = {
        cloned!(ctx, chunk, repo);
        async move {
            stream::iter_ok(chunk.clone())
                .map({
                    cloned!(ctx, repo);
                    move |cs_id| repo.get_bonsai_changeset(ctx.clone(), cs_id)
                })
                .buffer_unordered(100)
                .for_each(|_| Ok(()))
                .compat()
                .await
        }
    };

    let content_warmup = async {
        if PREFETCH_CONTENT_TYPES.contains(&derived_data_type.as_ref()) {
            content_warmup(ctx.clone(), repo.clone(), chunk.clone()).await?
        }
        Ok(())
    };

    let unode_warmup = async {
        if PREFETCH_UNODE_TYPES.contains(&derived_data_type.as_ref()) {
            unode_warmup(ctx.clone(), repo.clone(), &chunk).await?
        }
        Ok(())
    };

    try_join3(bcs_warmup, content_warmup, unode_warmup).await?;

    Ok(())
}

async fn content_warmup(
    ctx: CoreContext,
    repo: BlobRepo,
    chunk: Vec<ChangesetId>,
) -> Result<(), Error> {
    stream::iter_ok(chunk)
        .map({
            cloned!(ctx, repo);
            move |csid| prefetch_content(ctx.clone(), repo.clone(), csid)
        })
        .buffered(CHUNK_SIZE)
        .for_each(|_| Ok(()))
        .compat()
        .await
}

async fn unode_warmup(
    ctx: CoreContext,
    repo: BlobRepo,
    chunk: &Vec<ChangesetId>,
) -> Result<(), Error> {
    let unode_mapping = Arc::new(RootUnodeManifestMapping::new(repo.get_blobstore()));
    let futs = FuturesUnordered::new();
    for cs_id in chunk {
        cloned!(ctx, repo, unode_mapping);
        let f = async move {
            let bcs = repo
                .get_bonsai_changeset(ctx.clone(), *cs_id)
                .compat()
                .await?;

            let root_mf_id = RootUnodeManifestId::derive(
                ctx.clone(),
                repo.clone(),
                unode_mapping.clone(),
                bcs.get_changeset_id(),
            );

            let parent_unodes =
                fetch_parent_root_unodes(ctx.clone(), repo.clone(), bcs, unode_mapping.clone());
            let (root_mf_id, parent_unodes) =
                try_join!(root_mf_id.compat(), parent_unodes.compat())?;
            let unode_mf_id = root_mf_id.manifest_unode_id().clone();
            find_intersection_of_diffs(
                ctx.clone(),
                Arc::new(repo.get_blobstore()),
                unode_mf_id,
                parent_unodes,
            )
            .for_each(|_| Ok(()))
            .compat()
            .await
        };
        futs.push(f);
    }

    futs.try_for_each(|_| ready(Ok(()))).await
}

fn subcommand_tail(
    ctx: CoreContext,
    stats_constructor: impl Fn(&'static str) -> DerivedDataStats,
    repo: BlobRepo,
    unredacted_repo: BlobRepo,
    bookmarks: SqlBookmarks,
    derived_data_types: Vec<String>,
) -> impl Future<Item = (), Error = Error> {
    let derive_utils: Result<Vec<_>, Error> = derived_data_types
        .into_iter()
        .map(|name| {
            let maybe_unredacted_repo = if UNREDACTED_TYPES.contains(&name.as_ref()) {
                unredacted_repo.clone()
            } else {
                repo.clone()
            };
            let derive = derived_data_utils(ctx.clone(), repo.clone(), name)?;
            let stats = stats_constructor(derive.name());
            Ok((derive, maybe_unredacted_repo, Arc::new(stats)))
        })
        .collect();
    derive_utils.into_future().and_then(move |derive_utils| {
        let derive_utils = Arc::new(derive_utils);
        stream::repeat::<_, Error>(())
            .and_then(move |_| {
                bookmarks
                    .list_publishing_by_prefix(
                        ctx.clone(),
                        &BookmarkPrefix::empty(),
                        repo.get_repoid(),
                        Freshness::MostRecent,
                    )
                    .map(|(_name, csid)| csid)
                    .collect()
                    .and_then({
                        cloned!(ctx, derive_utils);
                        move |heads| {
                            let pending: Vec<_> = derive_utils
                                .iter()
                                .map({
                                    cloned!(ctx);
                                    move |(derive, maybe_unredacted_repo, stats)| {
                                        // create new context so each derivation would have its own trace
                                        let ctx = CoreContext::new_with_logger(
                                            ctx.fb,
                                            ctx.logger().clone(),
                                        );
                                        derive
                                            .pending(
                                                ctx.clone(),
                                                maybe_unredacted_repo.clone(),
                                                heads.clone(),
                                            )
                                            .map({
                                                cloned!(ctx, maybe_unredacted_repo, derive, stats);
                                                move |pending| {
                                                    stats
                                                        .pending_heads
                                                        .add_value(pending.len() as i64);
                                                    pending
                                                        .into_iter()
                                                        .map(|csid| {
                                                            derive.derive(
                                                                ctx.clone(),
                                                                maybe_unredacted_repo.clone(),
                                                                csid,
                                                            )
                                                        })
                                                        .collect::<Vec<_>>()
                                                }
                                            })
                                    }
                                })
                                .collect();

                            future::join_all(pending).and_then(move |pending| {
                                let pending: Vec<_> = pending.into_iter().flatten().collect();
                                if pending.is_empty() {
                                    tokio_timer::sleep(Duration::from_millis(250))
                                        .from_err()
                                        .left_future()
                                } else {
                                    let count = pending.len();
                                    info!(ctx.logger(), "found {} outdated heads", count);
                                    stream::iter_ok(pending)
                                        .buffered(1024)
                                        .for_each(|_| Ok(()))
                                        .timed({
                                            cloned!(ctx);
                                            move |stats, _| {
                                                info!(
                                                    ctx.logger(),
                                                    "derived data for {} heads in {:?}",
                                                    count,
                                                    stats.completion_time
                                                );
                                                Ok(())
                                            }
                                        })
                                        .right_future()
                                }
                            })
                        }
                    })
            })
            .for_each(|_| Ok(()))
    })
}

fn subcommand_single(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    derived_data_type: String,
) -> impl Future<Item = (), Error = Error> {
    let repo = repo.dangerous_override(|_| Arc::new(DummyLease {}) as Arc<dyn LeaseOps>);
    let derived_utils = match derived_data_utils(ctx.clone(), repo.clone(), derived_data_type) {
        Ok(derived_utils) => derived_utils,
        Err(error) => return future::err(error).left_future(),
    };
    derived_utils.regenerate(&vec![csid]);
    derived_utils
        .derive(ctx.clone(), repo, csid)
        .timed(move |stats, result| {
            info!(
                ctx.logger(),
                "derived in {:?}: {:?}", stats.completion_time, result
            );
            Ok(())
        })
        .map(|_| ())
        .right_future()
}

// Prefetch content of changed files between parents
fn prefetch_content(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
) -> impl Future<Item = (), Error = Error> {
    fn prefetch_content_unode(
        ctx: CoreContext,
        blobstore: Arc<dyn Blobstore>,
        renames: &HashMap<MPath, FileUnodeId>,
        path: MPath,
        file_unode_id: FileUnodeId,
    ) -> impl Future<Item = (), Error = Error> {
        let rename = renames.get(&path).copied();
        file_unode_id
            .load(ctx.clone(), &blobstore)
            .from_err()
            .and_then(move |file_unode| {
                let parents_content: Vec<_> = file_unode
                    .parents()
                    .iter()
                    .cloned()
                    .chain(rename)
                    .map({
                        cloned!(ctx, blobstore);
                        move |file_unode_id| {
                            fetch_file_full_content(ctx.clone(), blobstore.clone(), file_unode_id)
                        }
                    })
                    .collect();

                (
                    fetch_file_full_content(ctx.clone(), blobstore.clone(), file_unode_id),
                    future::join_all(parents_content),
                )
                    .into_future()
                    .map(|_| ())
            })
            .boxify()
    }

    csid.load(ctx.clone(), repo.blobstore())
        .from_err()
        .and_then(move |bonsai| {
            let unodes_mapping = Arc::new(RootUnodeManifestMapping::new(repo.get_blobstore()));
            let root_manifest = RootUnodeManifestId::derive(
                ctx.clone(),
                repo.clone(),
                unodes_mapping.clone(),
                csid,
            )
            .map(|mf| mf.manifest_unode_id().clone());

            let parents_manifest = bonsai.parents().collect::<Vec<_>>().into_iter().map({
                cloned!(ctx, repo);
                move |csid| {
                    RootUnodeManifestId::derive(
                        ctx.clone(),
                        repo.clone(),
                        unodes_mapping.clone(),
                        csid,
                    )
                    .map(|mf| mf.manifest_unode_id().clone())
                }
            });

            (
                root_manifest,
                future::join_all(parents_manifest),
                find_unode_renames(ctx.clone(), repo.clone(), &bonsai),
            )
                .into_future()
                .and_then(move |(root_mf, parents_mf, renames)| {
                    let blobstore = repo.get_blobstore().boxed();
                    find_intersection_of_diffs(ctx.clone(), blobstore.clone(), root_mf, parents_mf)
                        .filter_map(|(path, entry)| Some((path?, entry.into_leaf()?)))
                        .map(move |(path, file)| {
                            spawn_future(prefetch_content_unode(
                                ctx.clone(),
                                blobstore.clone(),
                                &renames,
                                path,
                                file,
                            ))
                        })
                        .buffered(256)
                        .for_each(|_| Ok(()))
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use blobstore::BlobstoreBytes;
    use fixtures::linear;
    use mercurial_types::HgChangesetId;
    use std::str::FromStr;
    use tokio_compat::runtime::Runtime;

    #[fbinit::test]
    fn test_backfill_data_latest(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = linear::getrepo(fb);
        let mut runtime = Runtime::new()?;

        let hg_cs_id = HgChangesetId::from_str("79a13814c5ce7330173ec04d279bf95ab3f652fb")?;
        let maybe_bcs_id = runtime.block_on(repo.get_bonsai_from_hg(ctx.clone(), hg_cs_id))?;
        let bcs_id = maybe_bcs_id.unwrap();

        let derived_utils =
            derived_data_utils(ctx.clone(), repo.clone(), RootUnodeManifestId::NAME)?;
        runtime.block_on(derived_utils.derive_batch(ctx.clone(), repo.clone(), vec![bcs_id]))?;

        Ok(())
    }

    #[fbinit::test]
    fn test_backfill_data_batch(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo = linear::getrepo(fb);
        let mut runtime = Runtime::new()?;

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

        let derived_utils =
            derived_data_utils(ctx.clone(), repo.clone(), RootUnodeManifestId::NAME)?;
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

        let ctx = CoreContext::test_mock(fb);
        let origrepo = linear::getrepo(fb);
        let mut runtime = Runtime::new()?;

        let repo = origrepo.dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
            Arc::new(FailingBlobstore::new("manifest".to_string(), blobstore))
        });

        let first_hg_cs_id = HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536")?;
        let maybe_bcs_id =
            runtime.block_on(repo.get_bonsai_from_hg(ctx.clone(), first_hg_cs_id))?;
        let bcs_id = maybe_bcs_id.unwrap();

        let derived_utils =
            derived_data_utils(ctx.clone(), repo.clone(), RootUnodeManifestId::NAME)?;
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
                tokio_timer::sleep(Duration::new(1, 0))
                    .from_err()
                    .and_then(|_| future::err(format_err!("failed")))
                    .boxify()
            } else {
                self.inner.put(ctx, key, value).boxify()
            }
        }

        fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Option<BlobstoreBytes>, Error> {
            self.inner.get(ctx, key)
        }
    }
}
