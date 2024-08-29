/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Range;
use std::sync::Arc;

use anyhow::Result;
use context::CoreContext;
use futures::stream;
use futures::Stream;
use futures::StreamExt;
use futures::TryStreamExt;
use mononoke_types::ChangesetId;

use crate::SqlCommitGraphStorage;

#[facet::facet]
pub struct CommitGraphBulkFetcher {
    storage: Arc<SqlCommitGraphStorage>,
}

/// A stream item returned by the bulk fetcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BulkFetcherStreamItem {
    /// The changeset id of the item.
    pub cs_id: ChangesetId,
    /// The auto-increment id of the item in the commit graph sql table.
    pub sql_id: u64,
}

/// The direction of the bulk fetcher stream.
#[derive(Clone, Copy)]
enum Direction {
    /// Iterate over changesets in increasing order of their auto-increment ids.
    OldestFirst,
    /// Iterate over changesets in decreasing order of their auto-increment ids.
    NewestFirst,
}

impl CommitGraphBulkFetcher {
    pub fn new(storage: Arc<SqlCommitGraphStorage>) -> Self {
        Self { storage }
    }

    /// Returns the bounds of the auto-increment ids of changesets in the repo.
    /// The bounds are returned as a half open interval [lo, hi).
    pub async fn repo_bounds(
        &self,
        ctx: &CoreContext,
        read_from_master: bool,
    ) -> Result<Range<u64>> {
        self.storage
            .repo_bounds(ctx, read_from_master)
            .await?
            .ok_or_else(|| anyhow::anyhow!("Can't get repo bounds for an empty repo"))
    }

    /// Returns a stream of all changeset ids in the repo that have auto-increment ids
    /// in the half open interval [lower_bound, upper_bound).`sql_chunk_size` specifies the
    /// number of changesets to fetch from sql in a single query.
    ///
    /// The stream is in increasing order of the auto-increment ids of the changesets.
    pub fn oldest_first_stream(
        &self,
        ctx: &CoreContext,
        range: Range<u64>,
        sql_chunk_size: u64,
        read_from_master: bool,
    ) -> impl Stream<Item = Result<BulkFetcherStreamItem>> {
        self.stream_impl(
            ctx,
            range,
            sql_chunk_size,
            read_from_master,
            Direction::OldestFirst,
        )
    }

    /// Returns a stream of all changeset ids in the repo that have auto-increment ids
    /// in the half open interval [lower_bound, upper_bound). `sql_chunk_size` specifies the
    /// number of changesets to fetch from sql in a single query.
    ///
    /// The stream is in decreasing order of the auto-increment ids of the changesets.
    pub fn newest_first_stream(
        &self,
        ctx: &CoreContext,
        range: Range<u64>,
        sql_chunk_size: u64,
        read_from_master: bool,
    ) -> impl Stream<Item = Result<BulkFetcherStreamItem>> {
        self.stream_impl(
            ctx,
            range,
            sql_chunk_size,
            read_from_master,
            Direction::NewestFirst,
        )
    }

    fn stream_impl(
        &self,
        ctx: &CoreContext,
        range: Range<u64>,
        sql_chunk_size: u64,
        read_from_master: bool,
        direction: Direction,
    ) -> impl Stream<Item = Result<BulkFetcherStreamItem>> {
        struct StreamState {
            storage: Arc<SqlCommitGraphStorage>,
            ctx: CoreContext,
            range: Range<u64>,
        }

        stream::try_unfold(
            StreamState {
                storage: self.storage.clone(),
                ctx: ctx.clone(),
                range,
            },
            move |state| async move {
                let StreamState {
                    storage,
                    ctx,
                    range,
                } = state;

                let (chunk_stream, new_lower_bound, new_upper_bound) = match direction {
                    Direction::OldestFirst => {
                        let chunk = storage
                            .fetch_oldest_changesets_in_range(
                                &ctx,
                                range.clone(),
                                sql_chunk_size,
                                read_from_master,
                            )
                            .await?;

                        if chunk.is_empty() {
                            return Ok(None);
                        }

                        let new_lower_bound = chunk
                            .iter()
                            .map(|(_cs_id, sql_id)| *sql_id + 1)
                            .max()
                            .unwrap_or(range.start);

                        let chunk_stream = stream::iter(chunk)
                            .map(|(cs_id, sql_id)| Ok(BulkFetcherStreamItem { cs_id, sql_id }));

                        (chunk_stream.left_stream(), new_lower_bound, range.end)
                    }
                    Direction::NewestFirst => {
                        let chunk = storage
                            .fetch_newest_changesets_in_range(
                                &ctx,
                                range.clone(),
                                sql_chunk_size,
                                read_from_master,
                            )
                            .await?;

                        if chunk.is_empty() {
                            return Ok(None);
                        }

                        let new_upper_bound = chunk
                            .iter()
                            .map(|(_cs_id, sql_id)| *sql_id)
                            .min()
                            .unwrap_or(range.end);

                        let chunk_stream = stream::iter(chunk)
                            .map(|(cs_id, sql_id)| Ok(BulkFetcherStreamItem { cs_id, sql_id }));

                        (chunk_stream.right_stream(), range.start, new_upper_bound)
                    }
                };

                anyhow::Ok(Some((
                    chunk_stream,
                    StreamState {
                        storage,
                        ctx,
                        range: new_lower_bound..new_upper_bound,
                    },
                )))
            },
        )
        .try_flatten()
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use commit_graph_testlib::utils::from_dag;
    use commit_graph_testlib::utils::name_cs_id;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;
    use mononoke_types::ChangesetId;
    use mononoke_types::RepositoryId;
    use rendezvous::RendezVousOptions;
    use sql_construct::SqlConstruct;

    use super::*;
    use crate::SqlCommitGraphStorageBuilder;

    async fn assert_stream(
        stream: impl Stream<Item = Result<BulkFetcherStreamItem>>,
        expected: Vec<(&str, u64)>,
    ) -> Result<()> {
        let actual = stream
            .map_ok(|item| (item.cs_id, item.sql_id))
            .try_collect::<Vec<_>>()
            .await?;

        let expected = expected
            .into_iter()
            .map(|(name, sql_id)| (name_cs_id(name), sql_id))
            .collect::<Vec<_>>();

        assert_eq!(actual, expected);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_commit_graph_bulk_fetcher(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let storage = Arc::new(
            SqlCommitGraphStorageBuilder::with_sqlite_in_memory()
                .unwrap()
                .build(RendezVousOptions::for_test(), RepositoryId::new(1)),
        );
        let graph = from_dag(
            &ctx,
            r"
            A-B-C-D-E-F-G-H-I-J-K
            ",
            storage.clone(),
        )
        .await?;

        let bulk_fetcher = CommitGraphBulkFetcher::new(storage.clone());

        assert_eq!(bulk_fetcher.repo_bounds(&ctx, false).await?, 1..12);

        assert_stream(
            bulk_fetcher.oldest_first_stream(&ctx, 1..12, 3, false),
            vec![
                ("A", 1),
                ("B", 2),
                ("C", 3),
                ("D", 4),
                ("E", 5),
                ("F", 6),
                ("G", 7),
                ("H", 8),
                ("I", 9),
                ("J", 10),
                ("K", 11),
            ],
        )
        .await?;

        assert_stream(
            bulk_fetcher.oldest_first_stream(&ctx, 5..8, 5, false),
            vec![("E", 5), ("F", 6), ("G", 7)],
        )
        .await?;

        assert_stream(
            bulk_fetcher.oldest_first_stream(&ctx, 6..6, 5, false),
            vec![],
        )
        .await?;

        assert_stream(
            bulk_fetcher.newest_first_stream(&ctx, 1..12, 2, false),
            vec![
                ("K", 11),
                ("J", 10),
                ("I", 9),
                ("H", 8),
                ("G", 7),
                ("F", 6),
                ("E", 5),
                ("D", 4),
                ("C", 3),
                ("B", 2),
                ("A", 1),
            ],
        )
        .await?;

        assert_stream(
            bulk_fetcher.newest_first_stream(&ctx, 0..6, 4, false),
            vec![("E", 5), ("D", 4), ("C", 3), ("B", 2), ("A", 1)],
        )
        .await?;

        assert_stream(
            bulk_fetcher.newest_first_stream(&ctx, 2..10, 2, false),
            vec![
                ("I", 9),
                ("H", 8),
                ("G", 7),
                ("F", 6),
                ("E", 5),
                ("D", 4),
                ("C", 3),
                ("B", 2),
            ],
        )
        .await?;

        Ok(())
    }
}
