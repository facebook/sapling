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
use buffered_commit_graph_storage::BufferedCommitGraphStorage;
use bulkops::Direction;
use bulkops::PublicChangesetBulkFetch;
use clap::Args;
use commit_graph::CommitGraph;
use context::CoreContext;
use futures::StreamExt;
use futures::TryStreamExt;
use futures_stats::TimedFutureExt;
use metaconfig_types::RepoConfigRef;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use phases::ArcPhases;
use phases::Phases;
use rendezvous::RendezVousOptions;
use repo_identity::RepoIdentityRef;
use sql_commit_graph_storage::SqlCommitGraphStorage;
use sql_commit_graph_storage::SqlCommitGraphStorageBuilder;

use super::Repo;

#[derive(Args)]
pub struct BackfillArgs {
    /// Which id to start backfilling from. Use 0 if nothing is backfilled.
    #[clap(long)]
    start_id: u64,

    /// The maximum number of changeset edges allowed to be stored in
    /// memory in the commit graph.
    #[clap(long)]
    max_in_memory_size: usize,

    /// The maximum number of changesets in a chunk fetched from
    /// the changesets table.
    #[clap(long)]
    chunk_size: usize,
}

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
    commit_graph: &CommitGraph,
    buffered_sql_storage: &BufferedCommitGraphStorage<SqlCommitGraphStorage>,
    repo: &Repo,
    args: BackfillArgs,
) -> Result<()> {
    let fetcher =
        PublicChangesetBulkFetch::new(repo.changesets.clone(), Arc::new(FakeAllCommitsPublic))
            .with_read_from_master(true);

    fetcher
        .fetch_bounded_with_id(ctx, Direction::OldestFirst, Some((args.start_id, u64::MAX)))
        .try_chunks(args.chunk_size)
        .map(|r| r.context("Error chunking"))
        .try_for_each(|entries| async move {
            println!("Doing chunk with {} entries", entries.len());

            if let (Some(first_entry), Some(last_entry)) = (entries.first(), entries.last()) {
                println!(
                    "Chunk starts at id {} and ends at id {}",
                    first_entry.1, last_entry.1
                );
            }

            let (stats, result) = async move {
                for (entry, _) in &entries {
                    commit_graph
                        .add(ctx, entry.cs_id, entry.parents.clone().into())
                        .await?;
                }
                Ok::<(), anyhow::Error>(())
            }
            .timed()
            .await;

            println!("Done in memory in {:?}", stats);
            result?;

            let (stats, result) = buffered_sql_storage.flush(ctx).timed().await;
            println!("Written to db in {:?}", stats);
            result?;

            Ok(())
        })
        .await?;

    println!("Backfilled everyting starting at id {}", args.start_id);
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
    let buffered_sql_storage = Arc::new(BufferedCommitGraphStorage::new(
        sql_storage,
        args.max_in_memory_size,
    ));
    let commit_graph = CommitGraph::new(buffered_sql_storage.clone());
    backfill_impl(ctx, &commit_graph, &buffered_sql_storage, repo, args).await
}
