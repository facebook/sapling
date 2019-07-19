// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::ArgMatches;
use cloned::cloned;
use failure_ext::Error;
use futures::future::{loop_fn, ok, Loop};
use futures::prelude::*;
use futures::stream::iter_ok;
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use rust_thrift::compact_protocol;
use std::collections::HashMap;
use std::sync::Arc;

use blobrepo::BlobRepo;
use blobstore::Blobstore;
use changeset_fetcher::ChangesetFetcher;
use changesets::{ChangesetEntry, Changesets, SqlChangesets};
use cmdlib::args;
use context::CoreContext;
use mononoke_types::{BlobstoreBytes, ChangesetId, Generation, RepositoryId};
use skiplist::{deserialize_skiplist_index, SkiplistIndex, SkiplistNodeType};
use slog::{debug, info, Logger};

use crate::cmdargs::{SKIPLIST_BUILD, SKIPLIST_READ};

pub fn subcommand_skiplist(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    match sub_m.subcommand() {
        (SKIPLIST_BUILD, Some(sub_m)) => {
            let key = sub_m
                .value_of("BLOBSTORE_KEY")
                .expect("blobstore key is not specified")
                .to_string();

            args::init_cachelib(&matches);
            let ctx = CoreContext::test_mock();
            let sql_changesets = args::open_sql::<SqlChangesets>(&matches);
            let repo = args::open_repo(&logger, &matches);
            repo.join(sql_changesets)
                .and_then(move |(repo, sql_changesets)| {
                    build_skiplist_index(ctx, repo, key, logger, sql_changesets)
                })
                .boxify()
        }
        (SKIPLIST_READ, Some(sub_m)) => {
            let key = sub_m
                .value_of("BLOBSTORE_KEY")
                .expect("blobstore key is not specified")
                .to_string();

            args::init_cachelib(&matches);
            let ctx = CoreContext::test_mock();
            args::open_repo(&logger, &matches)
                .and_then(move |repo| read_skiplist_index(ctx.clone(), repo, key, logger))
                .boxify()
        }
        _ => {
            println!("{}", sub_m.usage());
            ::std::process::exit(1);
        }
    }
}

fn build_skiplist_index<S: ToString>(
    ctx: CoreContext,
    repo: BlobRepo,
    key: S,
    logger: Logger,
    sql_changesets: SqlChangesets,
) -> BoxFuture<(), Error> {
    let blobstore = repo.get_blobstore();
    // skiplist will jump up to 2^9 changesets
    let skiplist_depth = 10;
    // Index all changesets
    let max_index_depth = 20000000000;
    let skiplist_index = SkiplistIndex::with_skip_edge_count(skiplist_depth);
    let key = key.to_string();

    let cs_fetcher = fetch_all_changesets(ctx.clone(), repo.get_repoid(), Arc::new(sql_changesets))
        .map({
            let changeset_fetcher = repo.get_changeset_fetcher();
            move |fetched_changesets| {
                let fetched_changesets: HashMap<_, _> = fetched_changesets
                    .into_iter()
                    .map(|cs_entry| (cs_entry.cs_id, cs_entry))
                    .collect();
                InMemoryChangesetFetcher {
                    fetched_changesets: Arc::new(fetched_changesets),
                    inner: changeset_fetcher,
                }
            }
        });

    repo.get_bonsai_heads_maybe_stale(ctx.clone())
        .collect()
        .join(cs_fetcher)
        .and_then({
            cloned!(ctx);
            move |(heads, cs_fetcher)| {
                loop_fn(
                    (heads.into_iter(), skiplist_index),
                    move |(mut heads, skiplist_index)| match heads.next() {
                        Some(head) => {
                            let f = skiplist_index.add_node(
                                ctx.clone(),
                                Arc::new(cs_fetcher.clone()),
                                head,
                                max_index_depth,
                            );

                            f.map(move |()| Loop::Continue((heads, skiplist_index)))
                                .boxify()
                        }
                        None => ok(Loop::Break(skiplist_index)).boxify(),
                    },
                )
            }
        })
        .inspect({
            cloned!(logger);
            move |skiplist_index| {
                info!(
                    logger,
                    "build {} skiplist nodes",
                    skiplist_index.indexed_node_count()
                );
            }
        })
        .map(|skiplist_index| {
            // We store only latest skip entry (i.e. entry with the longest jump)
            // This saves us storage space
            let mut thrift_merge_graph = HashMap::new();
            for (cs_id, skiplist_node_type) in skiplist_index.get_all_skip_edges() {
                let skiplist_node_type = if let SkiplistNodeType::SkipEdges(skip_edges) =
                    skiplist_node_type
                {
                    SkiplistNodeType::SkipEdges(skip_edges.last().cloned().into_iter().collect())
                } else {
                    skiplist_node_type
                };

                thrift_merge_graph.insert(cs_id.into_thrift(), skiplist_node_type.to_thrift());
            }
            compact_protocol::serialize(&thrift_merge_graph)
        })
        .and_then({
            cloned!(ctx);
            move |bytes| {
                debug!(logger, "storing {} bytes", bytes.len());
                blobstore.put(ctx, key, BlobstoreBytes::from_bytes(bytes))
            }
        })
        .boxify()
}

fn read_skiplist_index<S: ToString>(
    ctx: CoreContext,
    repo: BlobRepo,
    key: S,
    logger: Logger,
) -> BoxFuture<(), Error> {
    repo.get_blobstore()
        .get(ctx, key.to_string())
        .and_then(move |maybebytes| {
            match maybebytes {
                Some(bytes) => {
                    debug!(logger, "received {} bytes from blobstore", bytes.len());
                    let bytes = bytes.into_bytes();
                    let skiplist_index =
                        try_boxfuture!(deserialize_skiplist_index(logger.clone(), bytes));
                    info!(
                        logger,
                        "skiplist graph has {} entries",
                        skiplist_index.indexed_node_count()
                    );
                }
                None => {
                    println!("not found map");
                }
            };
            ok(()).boxify()
        })
        .boxify()
}

fn fetch_all_changesets(
    ctx: CoreContext,
    repo_id: RepositoryId,
    sqlchangesets: Arc<SqlChangesets>,
) -> impl Future<Item = Vec<ChangesetEntry>, Error = Error> {
    let num_sql_fetches = 10000;
    sqlchangesets
        .get_changesets_ids_bounds(repo_id.clone())
        .map(move |(maybe_lower_bound, maybe_upper_bound)| {
            let lower_bound = maybe_lower_bound.expect("changesets table is empty");
            let upper_bound = maybe_upper_bound.expect("changesets table is empty");
            let step = (upper_bound - lower_bound) / num_sql_fetches;
            let step = ::std::cmp::max(100, step);

            iter_ok(
                (lower_bound..upper_bound)
                    .step_by(step as usize)
                    .map(move |i| (i, i + step)),
            )
        })
        .flatten_stream()
        .and_then(move |(lower_bound, upper_bound)| {
            sqlchangesets
                .get_list_bs_cs_id_in_range(repo_id, lower_bound, upper_bound)
                .collect()
                .and_then({
                    cloned!(ctx, sqlchangesets);
                    move |ids| {
                        sqlchangesets
                            .get_many(ctx, repo_id, ids)
                            .map(|v| iter_ok(v.into_iter()))
                    }
                })
        })
        .flatten()
        .collect()
}

#[derive(Clone)]
struct InMemoryChangesetFetcher {
    fetched_changesets: Arc<HashMap<ChangesetId, ChangesetEntry>>,
    inner: Arc<dyn ChangesetFetcher>,
}

impl ChangesetFetcher for InMemoryChangesetFetcher {
    fn get_generation_number(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> BoxFuture<Generation, Error> {
        match self.fetched_changesets.get(&cs_id) {
            Some(cs_entry) => ok(Generation::new(cs_entry.gen)).boxify(),
            None => self.inner.get_generation_number(ctx, cs_id),
        }
    }

    fn get_parents(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> BoxFuture<Vec<ChangesetId>, Error> {
        match self.fetched_changesets.get(&cs_id) {
            Some(cs_entry) => ok(cs_entry.parents.clone()).boxify(),
            None => self.inner.get_parents(ctx, cs_id),
        }
    }
}
