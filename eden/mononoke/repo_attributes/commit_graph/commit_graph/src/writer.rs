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
use commit_graph_types::edges::ChangesetParents;
use context::CoreContext;
use futures_stats::TimedTryFutureExt;
use mononoke_types::ChangesetId;
use scuba_ext::MononokeScubaSampleBuilder;
use vec1::Vec1;

use crate::CommitGraph;
use crate::ParentsFetcher;

/// A trait for inserting new changesets to the commit graph.
#[facet::facet]
#[async_trait]
pub trait CommitGraphWriter {
    /// Add a new changeset to the commit graph.
    ///
    /// Returns true if a new changeset was inserted, or false if the
    /// changeset already existed.
    async fn add(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        parents: ChangesetParents,
    ) -> Result<bool>;

    /// Add many new changesets to the commit graph. Changesets should
    /// be sorted in topological order.
    ///
    /// Returns the number of newly added changesets to the commit graph.
    async fn add_many(
        &self,
        ctx: &CoreContext,
        changesets: Vec1<(ChangesetId, ChangesetParents)>,
    ) -> Result<usize>;

    /// Same as add but fetches parent edges using the changeset fetcher
    /// if not found in the storage, and recursively tries to add them.
    ///
    /// Changesets should be sorted in topological order.
    async fn add_recursive(
        &self,
        ctx: &CoreContext,
        parents_fetcher: Arc<dyn ParentsFetcher>,
        changesets: Vec1<(ChangesetId, ChangesetParents)>,
    ) -> Result<usize>;
}

#[derive(Clone)]
pub struct BaseCommitGraphWriter {
    commit_graph: CommitGraph,
}

impl BaseCommitGraphWriter {
    pub fn new(commit_graph: CommitGraph) -> Self {
        Self { commit_graph }
    }
}

#[async_trait]
impl CommitGraphWriter for BaseCommitGraphWriter {
    async fn add(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        parents: ChangesetParents,
    ) -> Result<bool> {
        self.commit_graph
            .add(ctx, cs_id, parents)
            .await
            .with_context(|| "during BaseCommitGraphWriter::add")
    }

    async fn add_many(
        &self,
        ctx: &CoreContext,
        changesets: Vec1<(ChangesetId, ChangesetParents)>,
    ) -> Result<usize> {
        self.commit_graph
            .add_many(ctx, changesets)
            .await
            .with_context(|| "during BaseCommitGraphWriter::add_many")
    }

    async fn add_recursive(
        &self,
        ctx: &CoreContext,
        parents_fetcher: Arc<dyn ParentsFetcher>,
        changesets: Vec1<(ChangesetId, ChangesetParents)>,
    ) -> Result<usize> {
        self.commit_graph
            .add_recursive(ctx, parents_fetcher, changesets)
            .await
            .with_context(|| "during BaseCommitGraphWriter::add_recursive")
    }
}

/// A wrapper around a commit graph writer that logs insertions to scuba.
pub struct LoggingCommitGraphWriter {
    inner_writer: ArcCommitGraphWriter,
    scuba: MononokeScubaSampleBuilder,
    repo_name: String,
}

impl LoggingCommitGraphWriter {
    pub fn new(
        inner_writer: ArcCommitGraphWriter,
        scuba: MononokeScubaSampleBuilder,
        repo_name: String,
    ) -> Self {
        Self {
            inner_writer,
            scuba,
            repo_name,
        }
    }
}

#[async_trait]
impl CommitGraphWriter for LoggingCommitGraphWriter {
    async fn add(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
        parents: ChangesetParents,
    ) -> Result<bool> {
        let mut scuba = self.scuba.clone();

        scuba.add_common_server_data();
        if let Some(client_info) = ctx.client_request_info() {
            scuba.add_client_request_info(client_info);
        }
        scuba.add("changeset_id", cs_id.to_string());
        scuba.add("changeset_count", 1);
        scuba.add("repo_name", self.repo_name.as_str());

        match self.inner_writer.add(ctx, cs_id, parents).try_timed().await {
            Err(err) => {
                scuba.add("error", err.to_string());
                scuba.log_with_msg("Insertion failed", None);

                Err(err)
            }
            Ok((stats, added_to_commit_graph)) => {
                scuba.add("time_s", stats.completion_time.as_secs_f64());
                scuba.add("num_added", added_to_commit_graph);

                if added_to_commit_graph {
                    scuba.log_with_msg("Insertion succeeded", None);
                } else {
                    scuba.log_with_msg("Changesets already stored", None);
                }

                Ok(added_to_commit_graph)
            }
        }
    }

    async fn add_many(
        &self,
        ctx: &CoreContext,
        changesets: Vec1<(ChangesetId, ChangesetParents)>,
    ) -> Result<usize> {
        let mut scuba = self.scuba.clone();

        scuba.add_common_server_data();
        if let Some(client_info) = ctx.client_request_info() {
            scuba.add_client_request_info(client_info);
        }
        // Only the last id, which is good enough for logging.
        scuba.add("changeset_id", changesets.last().0.to_string());
        scuba.add("changeset_count", changesets.len());
        scuba.add("repo_name", self.repo_name.as_str());

        match self
            .inner_writer
            .add_many(ctx, changesets)
            .try_timed()
            .await
        {
            Err(err) => {
                scuba.add("error", err.to_string());
                scuba.log_with_msg("Insertion failed", None);

                Err(err)
            }
            Ok((stats, added_to_commit_graph)) => {
                scuba.add("time_s", stats.completion_time.as_secs_f64());
                scuba.add("num_added", added_to_commit_graph);

                if added_to_commit_graph > 0 {
                    scuba.log_with_msg("Insertion succeeded", None);
                } else {
                    scuba.log_with_msg("Changesets already stored", None);
                }

                Ok(added_to_commit_graph)
            }
        }
    }

    async fn add_recursive(
        &self,
        ctx: &CoreContext,
        parents_fetcher: Arc<dyn ParentsFetcher>,
        changesets: Vec1<(ChangesetId, ChangesetParents)>,
    ) -> Result<usize> {
        let mut scuba = self.scuba.clone();

        scuba.add_common_server_data();
        if let Some(client_info) = ctx.client_request_info() {
            scuba.add_client_request_info(client_info);
        }
        // Only the last id, which is good enough for logging.
        scuba.add("changeset_id", changesets.last().0.to_string());
        scuba.add("changeset_count", changesets.len());
        scuba.add("repo_name", self.repo_name.as_str());

        // We use add_recursive because some parents might be missing
        // from the new commit graph.
        match self
            .inner_writer
            .add_recursive(ctx, parents_fetcher, changesets)
            .try_timed()
            .await
        {
            Err(err) => {
                scuba.add("error", err.to_string());
                scuba.log_with_msg("Insertion failed", None);

                Err(err)
            }
            Ok((stats, added_to_commit_graph)) => {
                scuba.add("time_s", stats.completion_time.as_secs_f64());
                scuba.add("num_added", added_to_commit_graph);

                if added_to_commit_graph > 0 {
                    scuba.log_with_msg("Insertion succeeded", None);
                } else {
                    scuba.log_with_msg("Changesets already stored", None);
                }

                Ok(added_to_commit_graph)
            }
        }
    }
}
