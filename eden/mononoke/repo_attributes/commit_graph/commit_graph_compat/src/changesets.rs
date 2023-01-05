/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::SimpleChangesetFetcher;
use changesets::ArcChangesets;
use changesets::ChangesetEntry;
use changesets::ChangesetInsert;
use changesets::Changesets;
use changesets::SortOrder;
use commit_graph::ArcCommitGraph;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream::BoxStream;
use futures_stats::TimedFutureExt;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::RepositoryId;
use scuba_ext::MononokeScubaSampleBuilder;
use smallvec::SmallVec;
use tunables::tunables;

pub struct ChangesetsCommitGraphCompat {
    changesets: ArcChangesets,
    changeset_fetcher: ArcChangesetFetcher,
    commit_graph: ArcCommitGraph,
    repo_name: String,
    scuba: MononokeScubaSampleBuilder,
}

impl ChangesetsCommitGraphCompat {
    pub fn new(
        fb: FacebookInit,
        changesets: ArcChangesets,
        commit_graph: ArcCommitGraph,
        repo_name: String,
        scuba_table: Option<&str>,
    ) -> Result<Self> {
        let changeset_fetcher = Arc::new(SimpleChangesetFetcher::new(
            changesets.clone(),
            changesets.repo_id(),
        ));

        let scuba = match scuba_table {
            Some(scuba_table) => MononokeScubaSampleBuilder::new(fb, scuba_table).with_context(
                || "Couldn't create scuba sample builder for table mononoke_commit_graph",
            )?,
            None => MononokeScubaSampleBuilder::with_discard(),
        };

        Ok(Self {
            changesets,
            changeset_fetcher,
            commit_graph,
            repo_name,
            scuba,
        })
    }
}

#[async_trait]
impl Changesets for ChangesetsCommitGraphCompat {
    fn repo_id(&self) -> RepositoryId {
        self.changesets.repo_id()
    }

    async fn add(&self, ctx: CoreContext, cs: ChangesetInsert) -> Result<bool, Error> {
        let added_to_changesets = self.changesets.add(ctx.clone(), cs.clone()).await?;

        if tunables()
            .get_by_repo_enable_writing_to_new_commit_graph(&self.repo_name)
            .unwrap_or(false)
        {
            let mut scuba = self.scuba.clone();

            scuba.add_common_server_data();
            scuba.add("changeset_id", cs.cs_id.to_string());
            scuba.add("repo_name", self.repo_name.as_str());

            let write_timeout = tunables().get_commit_graph_writes_timeout_ms() as u64;

            // We use add_recursive because some parents might be missing
            // from the new commit graph.
            match tokio::time::timeout(
                tokio::time::Duration::from_millis(write_timeout),
                self.commit_graph
                    .add_recursive(
                        &ctx,
                        self.changeset_fetcher.clone(),
                        cs.cs_id,
                        SmallVec::from_vec(cs.parents),
                    )
                    .timed(),
            )
            .await
            {
                Err(_) => {
                    scuba.add("timeout_ms", write_timeout);
                    scuba.log_with_msg("Insertion timed out", None);
                }
                Ok((stats, Err(err))) => {
                    scuba.add("error", err.to_string());
                    scuba.add("time_s", stats.completion_time.as_secs_f64());

                    scuba.log_with_msg("Insertion failed", None);
                }
                Ok((stats, Ok(added_to_commit_graph))) => {
                    scuba.add("time_s", stats.completion_time.as_secs_f64());
                    scuba.add("num_added", added_to_commit_graph);

                    if added_to_commit_graph > 0 {
                        scuba.log_with_msg("Insertion succeeded", None);
                    } else {
                        scuba.log_with_msg("Changeset already stored", None);
                    }
                }
            }
        }

        Ok(added_to_changesets)
    }

    async fn get(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEntry>, Error> {
        self.changesets.get(ctx, cs_id).await
    }

    async fn get_many(
        &self,
        ctx: CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetEntry>, Error> {
        self.changesets.get_many(ctx, cs_ids).await
    }

    async fn get_many_by_prefix(
        &self,
        ctx: CoreContext,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix, Error> {
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
    ) -> BoxStream<'_, Result<(ChangesetId, u64), Error>> {
        self.changesets.list_enumeration_range(
            ctx,
            min_id,
            max_id,
            sort_and_limit,
            read_from_master,
        )
    }
}
