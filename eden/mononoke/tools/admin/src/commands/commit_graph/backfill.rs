/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use bulkops::Direction;
use bulkops::PublicChangesetBulkFetch;
use clap::Args;
use commit_graph::CommitGraph;
use context::CoreContext;
use futures::TryStreamExt;
use metaconfig_types::RepoConfigRef;
use mononoke_app::MononokeApp;
use mononoke_types::ChangesetId;
use phases::ArcPhases;
use phases::Phases;
use rendezvous::RendezVousOptions;
use repo_identity::RepoIdentityRef;
use sql_commit_graph_storage::SqlCommitGraphStorageBuilder;

use super::Repo;

#[derive(Args)]
pub struct BackfillArgs {
    /// Which id to start backfilling from. Use 0 if nothing is backfilled.
    #[clap(long)]
    start_id: u64,
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
    repo: &Repo,
    args: BackfillArgs,
) -> Result<()> {
    let fetcher =
        PublicChangesetBulkFetch::new(repo.changesets.clone(), Arc::new(FakeAllCommitsPublic))
            .with_read_from_master(true);
    let mut done = 0;
    fetcher
        .fetch_bounded(ctx, Direction::OldestFirst, Some((args.start_id, u64::MAX)))
        .try_for_each(|entry| async move {
            done += 1;
            if done % 1000 == 0 {
                println!("Backfilled {} changesets", done);
            }
            commit_graph
                .add(ctx, entry.cs_id, entry.parents.into())
                .await?;
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
    let storage: SqlCommitGraphStorageBuilder = app
        .repo_factory()
        .sql_factory(&repo.repo_config().storage_config.metadata)
        .await?
        .open()?;
    let commit_graph = CommitGraph::new(Arc::new(storage.build(
        RendezVousOptions {
            free_connections: 5,
        },
        repo.repo_identity().id(),
    )));
    backfill_impl(ctx, &commit_graph, repo, args).await
}
