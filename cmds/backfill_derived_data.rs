// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use blobrepo::{BlobRepo, DangerousOverride};
use blobstore::Blobstore;
use bookmarks::{BookmarkPrefix, Bookmarks, Freshness};
use cacheblob::{dummy::DummyLease, LeaseOps, MemWritesBlobstore};
use changesets::{ChangesetEntry, Changesets, SqlChangesets};
use clap::{Arg, SubCommand};
use cloned::cloned;
use cmdlib::{args, monitoring::start_fb303_and_stats_agg};
use context::CoreContext;
use dbbookmarks::SqlBookmarks;
use derive_unode_manifest::derived_data_unodes::{RootUnodeManifestId, RootUnodeManifestMapping};
use derived_data::{BonsaiDerived, BonsaiDerivedMapping};
use failure::{err_msg, format_err};
use failure_ext::Error;
use fastlog::{RootFastlog, RootFastlogMapping};
use futures::{future, stream, Future, IntoFuture, Stream};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use futures_stats::Timed;
use lock_ext::LockExt;
use mononoke_types::{ChangesetId, MononokeId, RepositoryId};
use phases::SqlPhases;
use slog::info;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

const ARG_DERIVED_DATA_TYPE: &'static str = "DERIVED_DATA_TYPE";
const CHUNK_SIZE: usize = 4096;
const SUBCOMMAND_BACKFILL: &'static str = "backfill";
const SUBCOMMAND_TAIL: &'static str = "tail";

fn main() -> Result<(), Error> {
    let app = args::MononokeApp {
        hide_advanced_args: true,
        default_glog: false,
    }
    .build("Utility to work with bonsai derived data")
    .version("0.0.0")
    .about("Utility to work bonsai derived data")
    .subcommand(
        SubCommand::with_name(SUBCOMMAND_BACKFILL)
            .about("backfill derived data for public commits")
            .arg(
                Arg::with_name(ARG_DERIVED_DATA_TYPE)
                    .required(true)
                    .index(1)
                    .possible_values(&[RootUnodeManifestId::NAME])
                    .help("derived data type for which backfill will be run"),
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
                    .possible_values(&[RootUnodeManifestId::NAME])
                    .help("comma separated list of derived data types"),
            ),
    );
    let app = args::add_fb303_args(app);
    let matches = app.get_matches();
    args::init_cachelib(&matches);

    let logger = args::get_logger(&matches);
    let ctx = CoreContext::new_with_logger(logger.clone());
    let mut runtime = tokio::runtime::Runtime::new()?;

    let run = match matches.subcommand() {
        (SUBCOMMAND_BACKFILL, Some(sub_m)) => {
            let derived_data_type = sub_m
                .value_of(ARG_DERIVED_DATA_TYPE)
                .ok_or_else(|| format_err!("missing required argument: {}", ARG_DERIVED_DATA_TYPE))?
                .to_string();
            (
                args::open_repo(&logger, &matches),
                args::open_sql::<SqlChangesets>(&matches),
                args::open_sql::<SqlPhases>(&matches),
            )
                .into_future()
                .and_then(move |(repo, sql_changesets, sql_phases)| {
                    subcommand_backfill(ctx, repo, sql_changesets, sql_phases, derived_data_type)
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
            start_fb303_and_stats_agg(&mut runtime, &service_name, &logger, &matches)?;
            (
                args::open_repo(&logger, &matches),
                args::open_sql::<SqlBookmarks>(&matches),
            )
                .into_future()
                .and_then(move |(repo, bookmarks)| {
                    subcommand_tail(ctx, repo, bookmarks, derived_data_types)
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
    fn derive(&self, ctx: CoreContext, repo: BlobRepo, csid: ChangesetId) -> BoxFuture<(), Error>;

    /// Find pending changeset (changesets for which data have not been derived)
    fn pending(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetId>, Error>;
}

#[derive(Clone)]
struct DerivedUtilsFromMapping<M> {
    mapping: M,
}

impl<M> DerivedUtilsFromMapping<M> {
    fn new(mapping: M) -> Self {
        Self { mapping }
    }
}

impl<M> DerivedUtils for DerivedUtilsFromMapping<M>
where
    M: BonsaiDerivedMapping + Clone + 'static,
    M::Value: BonsaiDerived,
{
    fn derive(&self, ctx: CoreContext, repo: BlobRepo, csid: ChangesetId) -> BoxFuture<(), Error> {
        <M::Value as BonsaiDerived>::derive(ctx, repo, self.mapping.clone(), csid)
            .map(|_| ())
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
        name => Err(format_err!("Unsuppoerted derived data type: {}", name)),
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
            let start = start.ok_or_else(|| err_msg("changesets table is empty"))?;
            let stop = stop.ok_or_else(|| err_msg("changesets table is empty"))?;
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

fn subcommand_backfill(
    ctx: CoreContext,
    repo: BlobRepo,
    sql_changesets: SqlChangesets,
    sql_phases: SqlPhases,
    derived_data_type: String,
) -> BoxFuture<(), Error> {
    // Use `MemWritesBlobstore` to avoid blocking on writes to underlying blobstore.
    // `::preserve` is later used to bulk write all pending data.
    let mut memblobstore = None;
    let repo = repo
        .dangerous_override(|_| Arc::new(DummyLease {}) as Arc<dyn LeaseOps>)
        .dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
            let blobstore = Arc::new(MemWritesBlobstore::new(blobstore));
            memblobstore = Some(blobstore.clone());
            blobstore
        });
    let memblobstore = memblobstore.expect("memblobstore should have been updated");

    let derived_utils = try_boxfuture!(derived_data_utils(
        ctx.clone(),
        repo.clone(),
        derived_data_type
    ));

    println!("collecting all changest for: {:?}", repo.get_repoid());
    fetch_all_public_changesets(ctx.clone(), repo.get_repoid(), sql_changesets, sql_phases)
        .collect()
        .and_then(move |mut changesets| {
            changesets.sort_by_key(|cs_entry| cs_entry.gen);
            println!("starting deriving data for {} changesets", changesets.len());

            let total_count = changesets.len();
            let generated_count = Arc::new(AtomicUsize::new(0));
            let total_duration = Arc::new(Mutex::new(Duration::from_secs(0)));

            stream::iter_ok(changesets)
                .map(|entry| entry.cs_id)
                .chunks(CHUNK_SIZE)
                .and_then({
                    let blobstore = repo.get_blobstore();
                    cloned!(ctx, repo, derived_utils);
                    move |chunk| {
                        let changesets_prefetch = stream::iter_ok(chunk.clone())
                            .map({
                                cloned!(ctx, blobstore);
                                move |csid| blobstore.get(ctx.clone(), csid.blobstore_key())
                            })
                            .buffered(CHUNK_SIZE)
                            .for_each(|_| Ok(()));

                        (
                            changesets_prefetch,
                            derived_utils.pending(ctx.clone(), repo.clone(), chunk.clone()),
                        )
                            .into_future()
                            .map(move |(_, chunk)| chunk)
                    }
                })
                .for_each(move |chunk| {
                    let chunk_size = chunk.len();
                    stream::iter_ok(chunk)
                        .for_each({
                            cloned!(ctx, repo, derived_utils);
                            move |csid| derived_utils.derive(ctx.clone(), repo.clone(), csid)
                        })
                        .and_then({
                            cloned!(ctx, memblobstore);
                            move |()| memblobstore.persist(ctx)
                        })
                        .timed({
                            cloned!(generated_count, total_duration);
                            move |stats, _| {
                                generated_count.fetch_add(chunk_size, Ordering::SeqCst);
                                let elapsed = total_duration.with(|total_duration| {
                                    *total_duration += stats.completion_time;
                                    *total_duration
                                });

                                let generated = generated_count.load(Ordering::SeqCst) as f32;
                                let total = total_count as f32;
                                println!(
                                    "{}/{} estimate:{:.2?} speed:{:.2}/s mean_speed:{:.2}/s",
                                    generated,
                                    total_count,
                                    elapsed.mul_f32((total - generated) / generated),
                                    chunk_size as f32 / stats.completion_time.as_secs() as f32,
                                    generated / elapsed.as_secs() as f32,
                                );
                                Ok(())
                            }
                        })
                })
        })
        .boxify()
}

fn subcommand_tail(
    ctx: CoreContext,
    repo: BlobRepo,
    bookmarks: SqlBookmarks,
    derived_data_types: Vec<String>,
) -> impl Future<Item = (), Error = Error> {
    let derive_utils: Result<Vec<_>, Error> = derived_data_types
        .into_iter()
        .map(|name| derived_data_utils(ctx.clone(), repo.clone(), name))
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
                        cloned!(ctx, repo, derive_utils);
                        move |heads| {
                            let pending: Vec<_> = derive_utils
                                .iter()
                                .map({
                                    cloned!(ctx, repo);
                                    move |derive| {
                                        // create new context so each derivation would have its own trace
                                        let ctx =
                                            CoreContext::new_with_logger(ctx.logger().clone());
                                        derive
                                            .pending(ctx.clone(), repo.clone(), heads.clone())
                                            .map({
                                                cloned!(ctx, repo, derive);
                                                move |pending| {
                                                    pending
                                                        .into_iter()
                                                        .map(|csid| {
                                                            derive.derive(
                                                                ctx.clone(),
                                                                repo.clone(),
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
