/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::SimpleChangesetFetcher;
use changesets::ArcChangesets;
use changesets::ChangesetEntry;
use changesets::ChangesetInsert;
use changesets::Changesets;
use changesets::SortOrder;
use commit_graph::ArcCommitGraphWriter;
use context::CoreContext;
use futures::stream::BoxStream;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::RepositoryId;
use smallvec::SmallVec;
use vec1::vec1;
use vec1::Vec1;

pub struct ChangesetsCommitGraphCompat {
    changesets: ArcChangesets,
    changeset_fetcher: ArcChangesetFetcher,
    commit_graph_writer: ArcCommitGraphWriter,
}

impl ChangesetsCommitGraphCompat {
    pub fn new(
        changesets: ArcChangesets,
        commit_graph_writer: ArcCommitGraphWriter,
    ) -> Result<Self> {
        let changeset_fetcher = Arc::new(SimpleChangesetFetcher::new(
            changesets.clone(),
            changesets.repo_id(),
        ));

        Ok(Self {
            changesets,
            changeset_fetcher,
            commit_graph_writer,
        })
    }
}

#[async_trait]
impl Changesets for ChangesetsCommitGraphCompat {
    fn repo_id(&self) -> RepositoryId {
        self.changesets.repo_id()
    }

    async fn add(&self, ctx: &CoreContext, cs: ChangesetInsert) -> Result<bool> {
        let (added_to_changesets, _) = futures::try_join!(
            self.changesets.add(ctx, cs.clone()),
            self.commit_graph_writer.add_recursive(
                ctx,
                Arc::new(self.changeset_fetcher.clone()),
                vec1![(cs.cs_id, SmallVec::from_vec(cs.parents))],
            )
        )
        .with_context(|| "during commit_graph_compat::Changesets::add")?;
        Ok(added_to_changesets)
    }

    async fn add_many(&self, ctx: &CoreContext, css: Vec1<ChangesetInsert>) -> Result<()> {
        futures::try_join!(
            self.changesets.add_many(ctx, css.clone()),
            self.commit_graph_writer.add_recursive(
                ctx,
                Arc::new(self.changeset_fetcher.clone()),
                css.mapped(|cs| (cs.cs_id, SmallVec::from_vec(cs.parents))),
            )
        )
        .with_context(|| "during commit_graph_compat::Changesets::add")?;
        Ok(())
    }

    async fn get(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<Option<ChangesetEntry>> {
        self.changesets.get(ctx, cs_id).await
    }

    async fn get_many(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetEntry>> {
        self.changesets.get_many(ctx, cs_ids).await
    }

    async fn get_many_by_prefix(
        &self,
        ctx: &CoreContext,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix> {
        self.changesets
            .get_many_by_prefix(ctx, cs_prefix, limit)
            .await
    }

    fn prime_cache(&self, ctx: &CoreContext, changesets: &[ChangesetEntry]) {
        self.changesets.prime_cache(ctx, changesets)
    }

    async fn enumeration_bounds(
        &self,
        ctx: &CoreContext,
        read_from_master: bool,
        known_heads: Vec<ChangesetId>,
    ) -> Result<Option<(u64, u64)>> {
        self.changesets
            .enumeration_bounds(ctx, read_from_master, known_heads)
            .await
    }

    fn list_enumeration_range(
        &self,
        ctx: &CoreContext,
        min_id: u64,
        max_id: u64,
        sort_and_limit: Option<(SortOrder, u64)>,
        read_from_master: bool,
    ) -> BoxStream<'_, Result<(ChangesetId, u64)>> {
        self.changesets.list_enumeration_range(
            ctx,
            min_id,
            max_id,
            sort_and_limit,
            read_from_master,
        )
    }
}
