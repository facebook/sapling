/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Error;
use async_trait::async_trait;
use blobrepo::BlobRepo;
use blobstore::Blobstore;
use bookmarks::BookmarksMaybeStaleExt;
use bulkops::Direction;
use bulkops::PublicChangesetBulkFetch;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcher;
use changesets::ChangesetEntry;
use changesets::ChangesetsArc;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use context::CoreContext;
use context::SessionClass;
use fbinit::FacebookInit;
use fbthrift::compact_protocol;
use futures::future::try_join;
use futures::TryStreamExt;
use mononoke_types::BlobstoreBytes;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use phases::PhasesArc;
use skiplist::deserialize_skiplist_index;
use skiplist::sparse;
use skiplist::SkiplistIndex;
use skiplist::SkiplistNodeType;
use slog::debug;
use slog::info;
use slog::Logger;

use crate::error::SubcommandError;

pub const SKIPLIST: &str = "skiplist";
const SKIPLIST_BUILD: &str = "build";
const SKIPLIST_READ: &str = "read";
const ARG_EXPONENT: &str = "exponent";

// skiplist will jump up to 2^9 changesets
const DEFAULT_SKIPLIST_EXPONENT_STR: &str = "9";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(SKIPLIST)
        .about("commands to build or read skiplist indexes")
        .subcommand(
            SubCommand::with_name(SKIPLIST_BUILD)
                .about("build skiplist index")
                .arg(
                    Arg::with_name("BLOBSTORE_KEY")
                        .required(true)
                        .index(1)
                        .help("Blobstore key where to store the built skiplist"),
                )
                .arg(
                    Arg::with_name("rebuild")
                        .long("rebuild")
                        .help("forces the full rebuild instead of incremental update"),
                )
                .arg(
                    Arg::with_name(ARG_EXPONENT)
                        .long(ARG_EXPONENT)
                        .default_value(DEFAULT_SKIPLIST_EXPONENT_STR)
                        .help("Skiplist will skip up to 2^EXPONENT commits"),
                ),
        )
        .subcommand(
            SubCommand::with_name(SKIPLIST_READ)
                .about("read skiplist index")
                .arg(
                    Arg::with_name("BLOBSTORE_KEY")
                        .required(true)
                        .index(1)
                        .help("Blobstore key from where to read the skiplist"),
                ),
        )
}

pub async fn subcommand_skiplist<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    match sub_m.subcommand() {
        (SKIPLIST_BUILD, Some(sub_m)) => {
            let key = sub_m
                .value_of("BLOBSTORE_KEY")
                .expect("blobstore key is not specified")
                .to_string();
            let rebuild = sub_m.is_present("rebuild");
            let exponent = sub_m
                .value_of(ARG_EXPONENT)
                .expect("exponent must be set")
                .parse::<u32>()
                .map_err(Error::from)?;

            let mut ctx = CoreContext::new_with_logger(fb, logger.clone());
            // Set background session class so that skiplist building
            // completes fully.
            ctx.session_mut()
                .override_session_class(SessionClass::Background);
            let repo = args::not_shardmanager_compatible::open_repo(fb, &logger, matches).await?;
            build_skiplist_index(&ctx, &repo, key, &logger, rebuild, exponent)
                .await
                .map_err(SubcommandError::Error)
        }
        (SKIPLIST_READ, Some(sub_m)) => {
            let key = sub_m
                .value_of("BLOBSTORE_KEY")
                .expect("blobstore key is not specified")
                .to_string();

            let ctx = CoreContext::test_mock(fb);
            let repo = args::not_shardmanager_compatible::open_repo(fb, &logger, matches).await?;
            let maybe_index = read_skiplist_index(ctx.clone(), repo, key, logger.clone()).await?;
            match maybe_index {
                Some(index) => {
                    info!(
                        logger,
                        "skiplist graph has {} entries",
                        index.indexed_node_count()
                    );
                }
                None => {
                    info!(logger, "skiplist not found");
                }
            }
            Ok(())
        }
        _ => Err(SubcommandError::InvalidArgs),
    }
}

async fn build_skiplist_index<'a, S: ToString>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    key: S,
    logger: &'a Logger,
    force_full_rebuild: bool,
    exponent: u32,
) -> Result<(), Error> {
    let blobstore = repo.get_blobstore();
    // Depth must be one more than the maximum exponent.
    let skiplist_depth = exponent + 1;
    let key = key.to_string();
    let maybe_skiplist = if force_full_rebuild {
        None
    } else {
        read_skiplist_index(ctx.clone(), repo.clone(), key.clone(), logger.clone()).await?
    };

    let changeset_fetcher = repo.get_changeset_fetcher();
    let cs_fetcher_skiplist_func = async {
        match maybe_skiplist {
            Some(skiplist) => {
                info!(
                    logger,
                    "skiplist graph has {} entries",
                    skiplist.indexed_node_count()
                );
                Ok((changeset_fetcher, skiplist))
            }
            None => {
                info!(logger, "creating a skiplist from scratch");
                let skiplist_index = SkiplistIndex::with_skip_edge_count(skiplist_depth);
                let cs_fetcher =
                    fetch_all_public_changesets_and_build_changeset_fetcher(ctx, repo).await?;
                Ok((cs_fetcher, skiplist_index))
            }
        }
    };

    let heads = repo
        .bookmarks()
        .as_ref()
        .get_heads_maybe_stale(ctx.clone())
        .try_collect::<Vec<_>>();

    let (heads, (cs_fetcher, skiplist_index)) = try_join(heads, cs_fetcher_skiplist_func).await?;

    let updated_skiplist = {
        let mut index = skiplist_index.get_all_skip_edges();
        let max_skip = NonZeroU64::new(2u64.pow(skiplist_depth - 1))
            .ok_or_else(|| anyhow!("invalid skiplist depth"))?;
        sparse::update_sparse_skiplist(ctx, heads, &mut index, max_skip, &cs_fetcher).await?;
        index
    };

    info!(logger, "build {} skiplist nodes", updated_skiplist.len());

    // We store only latest skip entry (i.e. entry with the longest jump)
    // This saves us storage space
    let mut thrift_merge_graph = HashMap::new();
    for (cs_id, skiplist_node_type) in updated_skiplist {
        let skiplist_node_type = if let SkiplistNodeType::SkipEdges(skip_edges) = skiplist_node_type
        {
            SkiplistNodeType::SkipEdges(skip_edges.last().cloned().into_iter().collect())
        } else {
            skiplist_node_type
        };

        thrift_merge_graph.insert(cs_id.into_thrift(), skiplist_node_type.to_thrift());
    }
    let bytes = compact_protocol::serialize(&thrift_merge_graph);

    debug!(logger, "storing {} bytes", bytes.len());
    blobstore
        .put(ctx, key, BlobstoreBytes::from_bytes(bytes))
        .await
}

async fn fetch_all_public_changesets_and_build_changeset_fetcher(
    ctx: &CoreContext,
    repo: &BlobRepo,
) -> Result<ArcChangesetFetcher, Error> {
    let fetcher = PublicChangesetBulkFetch::new(repo.changesets_arc(), repo.phases_arc());
    let fetched_changesets = fetcher
        .fetch(ctx, Direction::OldestFirst)
        .try_collect::<Vec<_>>()
        .await?;

    let fetched_changesets: HashMap<_, _> = fetched_changesets
        .into_iter()
        .map(|cs_entry| (cs_entry.cs_id, cs_entry))
        .collect();
    let cs_fetcher: ArcChangesetFetcher = Arc::new(InMemoryChangesetFetcher {
        fetched_changesets: Arc::new(fetched_changesets),
        inner: repo.get_changeset_fetcher(),
    });

    Ok(cs_fetcher)
}

async fn read_skiplist_index<S: ToString>(
    ctx: CoreContext,
    repo: BlobRepo,
    key: S,
    logger: Logger,
) -> Result<Option<SkiplistIndex>, Error> {
    let key = key.to_string();
    let maybebytes = repo.blobstore().get(&ctx, &key).await?;
    match maybebytes {
        Some(bytes) => {
            debug!(
                logger,
                "received {} bytes from blobstore",
                bytes.as_bytes().len()
            );
            let bytes = bytes.into_raw_bytes();
            Ok(Some(deserialize_skiplist_index(logger.clone(), bytes)?))
        }
        None => Ok(None),
    }
}

#[derive(Clone)]
struct InMemoryChangesetFetcher {
    fetched_changesets: Arc<HashMap<ChangesetId, ChangesetEntry>>,
    inner: ArcChangesetFetcher,
}

#[async_trait]
impl ChangesetFetcher for InMemoryChangesetFetcher {
    async fn get_generation_number(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Generation, Error> {
        match self.fetched_changesets.get(&cs_id) {
            Some(cs_entry) => Ok(Generation::new(cs_entry.gen)),
            None => self.inner.get_generation_number(ctx, cs_id).await,
        }
    }

    async fn get_parents(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Vec<ChangesetId>, Error> {
        match self.fetched_changesets.get(&cs_id) {
            Some(cs_entry) => Ok(cs_entry.parents.clone()),
            None => self.inner.get_parents(ctx, cs_id).await,
        }
    }
}
