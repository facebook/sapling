/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::read::get_skiplist_index;
use super::Repo;
use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use bookmarks;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::Freshness;
use bulkops::Direction;
use bulkops::PublicChangesetBulkFetch;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcher;
use changeset_fetcher::ChangesetFetcherArc;
use changesets::ChangesetEntry;
use changesets::ChangesetsArc;
use clap::Args;
use context::CoreContext;
use fbthrift::compact_protocol;
use futures::future::try_join;
use futures::Stream;
use futures::TryStreamExt;
use mononoke_types::BlobstoreBytes;
use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use phases::PhasesArc;
use repo_blobstore::RepoBlobstoreRef;
use skiplist::sparse;
use skiplist::SkiplistIndex;
use skiplist::SkiplistNodeType;
use slog::debug;
use slog::info;
use slog::Logger;
use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Arc;

// skiplist will jump up to 2^9 changesets.
const DEFAULT_SKIPLIST_EXPONENT_STR: &str = "9";

#[derive(Args)]
/// Subcommand to build skiplist indexes.
pub struct SkiplistBuildArgs {
    /// When set, forces the full rebuild instead of
    /// incremental update.
    #[clap(long, short = 'r')]
    rebuild: bool,

    /// Skiplist will skip up to 2^EXPONENT commits.
    #[clap(long, short = 'e', default_value = DEFAULT_SKIPLIST_EXPONENT_STR)]
    exponent: String,
}

pub async fn build_skiplist(
    ctx: &CoreContext,
    repo: &Repo,
    logger: &Logger,
    blobstore_key: String,
    args: SkiplistBuildArgs,
) -> Result<()> {
    let exponent = args.exponent.parse::<u32>().map_err(Error::from)?;
    // Depth must be one more than the maximum exponent.
    let skiplist_depth = exponent + 1;
    let maybe_skiplist = if args.rebuild {
        None
    } else {
        get_skiplist_index(ctx, repo, logger, blobstore_key.clone()).await?
    };

    let changeset_fetcher = repo.changeset_fetcher_arc();
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

    let heads = get_bonsai_heads_maybe_stale(repo, ctx.clone()).try_collect::<Vec<_>>();

    let (heads, (cs_fetcher, skiplist_index)) = try_join(heads, cs_fetcher_skiplist_func).await?;

    let updated_skiplist = {
        let mut index = skiplist_index.get_all_skip_edges();
        let max_skip = NonZeroU64::new(2u64.pow(skiplist_depth - 1))
            .ok_or_else(|| anyhow!("invalid skiplist depth"))?;
        sparse::update_sparse_skiplist(ctx, heads, &mut index, max_skip, &cs_fetcher).await?;
        index
    };

    info!(logger, "built {} skiplist nodes", updated_skiplist.len());

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
    repo.repo_blobstore()
        .put(
            ctx,
            blobstore_key.clone(),
            BlobstoreBytes::from_bytes(bytes),
        )
        .await
}

async fn fetch_all_public_changesets_and_build_changeset_fetcher(
    ctx: &CoreContext,
    repo: &Repo,
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
        inner: repo.changeset_fetcher_arc(),
    });

    Ok(cs_fetcher)
}

/// Get Bonsai changesets for Mercurial heads, which we approximate as Publishing Bonsai
/// Bookmarks. Those will be served from cache, so they might be stale.
pub fn get_bonsai_heads_maybe_stale(
    repo: &Repo,
    ctx: CoreContext,
) -> impl Stream<Item = Result<ChangesetId, Error>> {
    repo.bookmarks
        .list(
            ctx,
            Freshness::MaybeStale,
            &BookmarkPrefix::empty(),
            BookmarkKind::ALL_PUBLISHING,
            &BookmarkPagination::FromStart,
            std::u64::MAX,
        )
        .map_ok(|(_, cs_id)| cs_id)
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
