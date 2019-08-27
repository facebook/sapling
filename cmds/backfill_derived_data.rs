// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(duration_float)]

use blobrepo::DangerousOverride;
use blobstore::Blobstore;
use cacheblob::{dummy::DummyLease, LeaseOps, MemWritesBlobstore};
use changesets::{ChangesetEntry, Changesets, SqlChangesets};
use clap::Arg;
use cloned::cloned;
use cmdlib::args;
use context::CoreContext;
use derive_unode_manifest::derived_data_unodes::{RootUnodeManifestId, RootUnodeManifestMapping};
use derived_data::{BonsaiDerived, BonsaiDerivedMapping};
use failure::{err_msg, format_err};
use failure_ext::Error;
use futures::{stream, Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, FutureExt};
use futures_stats::Timed;
use lock_ext::LockExt;
use mononoke_types::{ChangesetId, MononokeId, RepositoryId};
use phases::SqlPhases;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};

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
    changesets: Arc<SqlChangesets>,
    phases: Arc<SqlPhases>,
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

const CHUNK_SIZE: usize = 4096;

type DeriveFn = Arc<dyn Fn(ChangesetId) -> BoxFuture<(), Error> + Send + Sync + 'static>;
type DerivePrefetchFn =
    Arc<dyn Fn(Vec<ChangesetId>) -> BoxFuture<(), Error> + Send + Sync + 'static>;

fn main() -> Result<(), Error> {
    let matches = args::MononokeApp {
        safe_writes: false,
        hide_advanced_args: true,
        default_glog: false,
    }
    .build("Utility to back-fill bonsai derived data")
    .version("0.0.0")
    .about("Utility to back-fill bonsai derived data")
    .arg(
        Arg::with_name("DERIVED_DATA_TYPE")
            .required(true)
            .index(1)
            .possible_values(&[RootUnodeManifestId::NAME])
            .help("derived data type for which backfill will be run"),
    )
    .get_matches();

    args::init_cachelib(&matches);

    let mut runtime = tokio::runtime::Runtime::new()?;
    let ctx = CoreContext::test_mock();

    let logger = args::get_logger(&matches);

    // Use `MemWritesBlobstore` to avoid blocking on writes to underlying blobstore.
    // `::preserve` is later used to bulk write all pending data.
    let mut memblobstore = None;
    let repo = runtime
        .block_on(args::open_repo(&logger, &matches))?
        .dangerous_override(|_| Arc::new(DummyLease {}) as Arc<dyn LeaseOps>)
        .dangerous_override(|blobstore| -> Arc<dyn Blobstore> {
            let blobstore = Arc::new(MemWritesBlobstore::new(blobstore));
            memblobstore = Some(blobstore.clone());
            blobstore
        });
    let memblobstore = memblobstore.expect("memblobstore should have been updated");

    let sql_changesets = Arc::new(runtime.block_on(args::open_sql::<SqlChangesets>(&matches))?);
    let sql_phases = Arc::new(runtime.block_on(args::open_sql::<SqlPhases>(&matches))?);

    let (derive, derive_prefetch): (DeriveFn, DerivePrefetchFn) = match matches
        .value_of("DERIVED_DATA_TYPE")
    {
        Some(RootUnodeManifestId::NAME) => {
            // TODO: we should probably add generic layer of caching on top of mapping
            //       which will store changesets one after another, to make it resilient
            //       to interrupts. Otherwise `MwmWriterBlobstore` can crate mapping entry
            //       before storing whole derived data.
            let mapping = Arc::new(RootUnodeManifestMapping::new(repo.get_blobstore()));
            let derive_unodes = {
                cloned!(ctx, repo, mapping);
                move |csid| {
                    RootUnodeManifestId::derive(ctx.clone(), repo.clone(), mapping.clone(), csid)
                        .map(|_| ())
                        .boxify()
                }
            };
            let derive_prefetch_unodes = {
                cloned!(ctx, mapping);
                move |csids| mapping.get(ctx.clone(), csids).map(|_| ()).boxify()
            };
            (Arc::new(derive_unodes), Arc::new(derive_prefetch_unodes))
        }
        unsupported_type => {
            return Err(format_err!(
                "Unsupported derived data type: {:?}",
                unsupported_type
            ));
        }
    };

    println!("collecting all changest for: {:?}", repo.get_repoid());
    runtime.block_on(
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
                        cloned!(ctx);
                        move |chunk| {
                            let changesets_prefetch = stream::iter_ok(chunk.clone())
                                .map({
                                    cloned!(ctx, blobstore);
                                    move |csid| blobstore.get(ctx.clone(), csid.blobstore_key())
                                })
                                .buffered(CHUNK_SIZE)
                                .for_each(|_| Ok(()));

                            (changesets_prefetch, derive_prefetch(chunk.clone()))
                                .into_future()
                                .map(move |_| chunk)
                        }
                    })
                    .for_each(move |chunk| {
                        let chunk_size = chunk.len();
                        stream::iter_ok(chunk)
                            .for_each({
                                cloned!(derive);
                                move |csid| derive(csid)
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
            }),
    )?;

    Ok(())
}
