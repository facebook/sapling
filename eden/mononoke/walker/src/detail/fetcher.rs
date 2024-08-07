/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Range;

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use futures::stream::BoxStream;
use futures::StreamExt;
use futures::TryStreamExt;
use mononoke_types::ChangesetId;
use sql_commit_graph_storage::CommitGraphBulkFetcher;
use strum::AsRefStr;
use strum::EnumString;
use strum::VariantNames;

pub const MAX_FETCH_STEP: u64 = 1000;

/// A trait for iterating over changesets in a repository using
/// their sql auto-increment ids.
#[async_trait]
pub trait BulkFetcherOps: Send + Sync {
    /// Returns the bounds of the auto-increment ids of changesets in the repo.
    /// The bounds are returned as a half open interval [lo, hi).
    async fn repo_bounds(&self, ctx: &CoreContext) -> Result<Range<u64>>;

    /// Returns a stream of all changeset ids in the repo that have auto-increment ids
    /// in the half open interval [lower_bound, upper_bound).`sql_chunk_size` specifies the
    /// number of changesets to fetch from sql in a single query.
    ///
    /// If `direction` is set to `Direction::OldestFirst` then the stream is increasing order of the auto-increment ids of the changesets,
    /// otherwise if it's `Direction::NewestFirst` then it is in decreasing order.
    fn changesets_stream<'a>(
        &'a self,
        ctx: &'a CoreContext,
        direction: Direction,
        bounds: Range<u64>,
    ) -> BoxStream<'a, Result<(ChangesetId, u64)>>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, AsRefStr, VariantNames, EnumString)]
pub enum Direction {
    NewestFirst,
    OldestFirst,
}

#[async_trait]
impl BulkFetcherOps for CommitGraphBulkFetcher {
    async fn repo_bounds(&self, ctx: &CoreContext) -> Result<Range<u64>> {
        self.repo_bounds(ctx, false).await
    }

    fn changesets_stream<'a>(
        &'a self,
        ctx: &'a CoreContext,
        direction: Direction,
        bounds: Range<u64>,
    ) -> BoxStream<'a, Result<(ChangesetId, u64)>> {
        match direction {
            Direction::OldestFirst => self
                .oldest_first_stream(ctx, bounds, MAX_FETCH_STEP, false)
                .map_ok(|item| (item.cs_id, item.sql_id))
                .boxed(),
            Direction::NewestFirst => self
                .newest_first_stream(ctx, bounds, MAX_FETCH_STEP, false)
                .map_ok(|item| (item.cs_id, item.sql_id))
                .boxed(),
        }
    }
}
