/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use bulkops::Direction;
use bulkops::PublicChangesetBulkFetch;
use clap::Args;
use commit_graph::storage::CommitGraphStorage;
use commit_graph::CommitGraph;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures_stats::TimedFutureExt;
use in_memory_commit_graph_storage::InMemoryCommitGraphStorage;
use metaconfig_types::RepoConfigRef;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use phases::ArcPhases;
use phases::Phases;
use rendezvous::RendezVousOptions;
use repo_identity::RepoIdentityRef;
use sql_commit_graph_storage::SqlCommitGraphStorage;
use sql_commit_graph_storage::SqlCommitGraphStorageBuilder;
use vec1::Vec1;

use super::Repo;

#[derive(Args)]
pub struct BackfillArgs {}

// We need to pretend all commits are public because we want to backfill all
struct FakeAllCommitsPublic;

#[async_trait]
impl Phases for FakeAllCommitsPublic {
    async fn add_reachable_as_public(
        &self,
        _ctx: &CoreContext,
        _heads: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetId>> {
        unimplemented!()
    }

    async fn add_public_with_known_public_ancestors(
        &self,
        _ctx: &CoreContext,
        _csids: Vec<ChangesetId>,
    ) -> Result<()> {
        unimplemented!()
    }

    async fn get_public(
        &self,
        _ctx: &CoreContext,
        csids: Vec<ChangesetId>,
        _ephemeral_derive: bool,
    ) -> Result<HashSet<ChangesetId>> {
        Ok(csids.into_iter().collect())
    }

    async fn get_cached_public(
        &self,
        _ctx: &CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<HashSet<ChangesetId>> {
        Ok(csids.into_iter().collect())
    }

    async fn list_all_public(&self, _ctx: &CoreContext) -> Result<Vec<ChangesetId>> {
        unimplemented!()
    }

    fn with_frozen_public_heads(&self, _heads: Vec<ChangesetId>) -> ArcPhases {
        unimplemented!()
    }
}

async fn backfill_impl(
    ctx: &CoreContext,
    commit_graph_in_memory: &CommitGraph,
    in_memory_storage: &InMemoryCommitGraphStorage,
    sql_storage: &SqlCommitGraphStorage,
    repo: &Repo,
    args: BackfillArgs,
) -> Result<()> {
    let BackfillArgs {} = args;
    let fetcher =
        PublicChangesetBulkFetch::new(repo.changesets.clone(), Arc::new(FakeAllCommitsPublic))
            .with_read_from_master(true);
    fetcher
        .fetch_bounded(ctx, Direction::OldestFirst, Some((0, u64::MAX)))
        .try_chunks(5_000)
        .map(|r| r.context("Error chunking"))
        .try_for_each(|entries| async move {
            println!("Doing chunk with {} entries", entries.len());
            let (stats, result) = async move {
                for entry in &entries {
                    commit_graph_in_memory
                        .add(ctx, entry.cs_id, entry.parents.clone().into())
                        .await?;
                }
                let mut all_edges = in_memory_storage
                    .fetch_many_edges_required(
                        ctx,
                        &entries.iter().map(|e| e.cs_id).collect::<Vec<_>>(),
                        None,
                    )
                    .await?;
                entries
                    .into_iter()
                    .map(|entry| {
                        all_edges
                            .remove(&entry.cs_id)
                            .with_context(|| format!("Missing changeset {}", entry.cs_id))
                    })
                    .collect::<Result<Vec<_>>>()
            }
            .timed()
            .await;
            println!("Done in memory in {:?}", stats);
            let edges_to_add = result?;
            if let Ok(edges_to_add) = Vec1::try_from_vec(edges_to_add) {
                let (stats, result) = sql_storage.add_many(ctx, edges_to_add).timed().await;
                println!("Written to db in {:?}", stats);
                result?;
            }
            Ok(())
        })
        .await?;
    println!("Backfilled everyting starting at id 0");
    Ok(())
}

pub(super) async fn backfill(
    ctx: &CoreContext,
    app: &MononokeApp,
    repo: &Repo,
    args: BackfillArgs,
) -> Result<()> {
    let sql_storage = app
        .repo_factory()
        .sql_factory(&repo.repo_config().storage_config.metadata)
        .await?
        .open::<SqlCommitGraphStorageBuilder>()?
        .build(
            RendezVousOptions {
                free_connections: 5,
            },
            repo.repo_identity().id(),
        );
    let in_memory_storage = Arc::new(InMemoryCommitGraphStorage::new(repo.repo_identity().id()));
    let commit_graph = CommitGraph::new(in_memory_storage.clone());
    backfill_impl(
        ctx,
        &commit_graph,
        &in_memory_storage,
        &sql_storage,
        repo,
        args,
    )
    .await
}
