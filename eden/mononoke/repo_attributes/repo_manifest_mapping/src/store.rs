/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_ext::Connection;
use sql_ext::SqlConnections;
use sql_ext::mononoke_queries;

use crate::RepoManifestMapping;
use crate::Staleness;
use crate::types::ManifestBranch;
use crate::types::MembershipEdge;
use crate::types::RepoBranch;
use crate::types::RepoName;

mononoke_queries! {
    // Hot reverse / fan-out read: which manifest branches (across all manifest
    // repos) include the given member repo on the given repo branch. Results
    // are DISTINCT (defensive: the UNIQUE key already forbids duplicate rows,
    // so a given (manifest_repo_id, manifest_branch) cannot appear twice for a
    // fixed (repo_name, repo_branch)). ORDER BY makes the output sequence a
    // contract rather than an incidental index-scan artifact, so the in-memory
    // Test double (which sorts) is an observationally faithful mirror.
    read GetManifestBranchesForRepo(
        repo_name: RepoName,
        repo_branch: RepoBranch,
    ) -> (RepositoryId, ManifestBranch) {
        "SELECT DISTINCT manifest_repo_id, manifest_branch
         FROM repo_manifest_mapping
         WHERE repo_name = {repo_name} AND repo_branch = {repo_branch}
         ORDER BY manifest_repo_id, manifest_branch"
    }

    // Forward read: all member repos of a manifest branch, scoped to the
    // owning manifest repo. ORDER BY makes the output order contractual (see
    // the reverse read above).
    read GetMembersForManifestBranch(
        manifest_repo_id: RepositoryId,
        manifest_branch: ManifestBranch,
    ) -> (RepoName, RepoBranch) {
        "SELECT repo_name, repo_branch
         FROM repo_manifest_mapping
         WHERE manifest_repo_id = {manifest_repo_id} AND manifest_branch = {manifest_branch}
         ORDER BY repo_name, repo_branch"
    }

    // Bulk insert of membership edges. Plain INSERT (not INSERT OR IGNORE):
    // the replace flow deletes the manifest branch's rows first, so there is
    // nothing to conflict with, and `replace_membership` de-duplicates the batch
    // before inserting, so the VALUES list is always UNIQUE-key-clean.
    write InsertEdges(values: (
        manifest_repo_id: RepositoryId,
        manifest_branch: ManifestBranch,
        repo_name: RepoName,
        repo_branch: RepoBranch,
    )) {
        none,
        "INSERT INTO repo_manifest_mapping (manifest_repo_id, manifest_branch, repo_name, repo_branch) VALUES {values}"
    }

    write DeleteEdgesForManifestBranch(
        manifest_repo_id: RepositoryId,
        manifest_branch: ManifestBranch,
    ) {
        none,
        "DELETE FROM repo_manifest_mapping WHERE manifest_repo_id = {manifest_repo_id} AND manifest_branch = {manifest_branch}"
    }

    read GetWatermark(repo_id: RepositoryId) -> (i64,) {
        "SELECT log_id FROM manifest_watermark WHERE repo_id = {repo_id}"
    }

    // Unconditional upsert, deliberately NOT a compare-and-swap. Exactly-once is
    // delivered by advancing the watermark in the SAME transaction as the
    // membership replace (see `replace_membership`), and the tailer that owns
    // this watermark is a single-leader singleton per manifest repo. A CAS guard
    // would only add defense against concurrent/split-brain writers, which that
    // model precludes; add it if a future consumer needs it.
    write SetWatermark(repo_id: RepositoryId, log_id: i64) {
        none,
        "REPLACE INTO manifest_watermark (repo_id, log_id) VALUES ({repo_id}, {log_id})"
    }
}

/// A GLOBAL/shared store: it carries only its connections. Per-call telemetry
/// is threaded through the `CoreContext` (like `git_source_of_truth`), not
/// stored on the struct.
pub struct SqlRepoManifestMapping {
    connections: SqlConnections,
}

impl SqlRepoManifestMapping {
    fn get_connection(&self, staleness: Staleness) -> &Connection {
        match staleness {
            Staleness::MostRecent => &self.connections.read_master_connection,
            Staleness::MaybeStale => &self.connections.read_connection,
        }
    }
}

/// Builds a [`SqlRepoManifestMapping`] from `SqlConnections`. The store is a
/// plain global store handed around as `Arc<dyn RepoManifestMapping>` and
/// constructed directly by each consuming job/service, so `SqlConstruct` is all
/// that is wired up here.
#[derive(Clone)]
pub struct SqlRepoManifestMappingBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlRepoManifestMappingBuilder {
    const LABEL: &'static str = "repo_manifest_mapping";

    const CREATION_QUERY: &'static str = concat!(
        include_str!("../schemas/sqlite-repo-manifest-mapping.sql"),
        include_str!("../schemas/sqlite-manifest-watermark.sql"),
    );

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

// NOTE: `SqlConstructFromMetadataDatabaseConfig` is deliberately NOT
// implemented here. Wiring this store into the metadata database config (and
// the corresponding metaconfig field) is scoped to a follow-up.

impl SqlRepoManifestMappingBuilder {
    /// Consume the builder and produce the ready-to-use store.
    pub fn build(self) -> SqlRepoManifestMapping {
        let SqlRepoManifestMappingBuilder { connections } = self;
        SqlRepoManifestMapping { connections }
    }
}

#[async_trait]
impl RepoManifestMapping for SqlRepoManifestMapping {
    async fn manifest_branches_for_repo(
        &self,
        ctx: &CoreContext,
        repo_name: &RepoName,
        repo_branch: &RepoBranch,
        staleness: Staleness,
    ) -> Result<Vec<(RepositoryId, ManifestBranch)>> {
        let rows = GetManifestBranchesForRepo::query(
            self.get_connection(staleness),
            ctx.sql_query_telemetry(),
            repo_name,
            repo_branch,
        )
        .await
        .with_context(|| {
            format!("Failure fetching manifest branches for repo {repo_name} branch {repo_branch}")
        })?;
        Ok(rows)
    }

    async fn members_for_manifest_branch(
        &self,
        ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        manifest_branch: &ManifestBranch,
        staleness: Staleness,
    ) -> Result<Vec<MembershipEdge>> {
        let rows = GetMembersForManifestBranch::query(
            self.get_connection(staleness),
            ctx.sql_query_telemetry(),
            &manifest_repo_id,
            manifest_branch,
        )
        .await
        .with_context(|| {
            format!(
                "Failure fetching members for manifest repo {manifest_repo_id} branch {manifest_branch}"
            )
        })?;
        Ok(rows
            .into_iter()
            .map(|(repo_name, repo_branch)| MembershipEdge::new(repo_name, repo_branch))
            .collect())
    }

    async fn replace_membership(
        &self,
        ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        manifest_branch: &ManifestBranch,
        edges: &[MembershipEdge],
        watermark: Option<i64>,
    ) -> Result<()> {
        // De-duplicate the batch: membership is a SET, and a real manifest can
        // legitimately list the same (repo_name, repo_branch) more than once (e.g.
        // the same repo pinned at the same branch via two different project paths —
        // the path is not part of the edge). Collapsing duplicates keeps the bulk
        // INSERT free of UNIQUE-key conflicts and matches set semantics; the
        // in-memory Test double dedups identically.
        let mut seen = std::collections::HashSet::with_capacity(edges.len());
        let deduped: Vec<&MembershipEdge> = edges.iter().filter(|e| seen.insert(*e)).collect();

        let mut txn = self
            .connections
            .write_connection
            .start_transaction(ctx.sql_query_telemetry())
            .await
            .with_context(|| {
                format!(
                    "Failed to start transaction replacing membership for manifest repo {manifest_repo_id} branch {manifest_branch}"
                )
            })?;

        let (txn_, _) = DeleteEdgesForManifestBranch::query_with_transaction(
            txn,
            &manifest_repo_id,
            manifest_branch,
        )
        .await
        .with_context(|| {
            format!(
                "Failed to delete existing edges for manifest repo {manifest_repo_id} branch {manifest_branch}"
            )
        })?;
        txn = txn_;

        // Skip the INSERT entirely when there is nothing to insert: an empty
        // `VALUES` clause would be invalid SQL. An empty `edges` slice is a
        // legitimate request to clear the manifest branch's membership.
        if !deduped.is_empty() {
            let rows: Vec<_> = deduped
                .iter()
                .copied()
                .map(|edge| {
                    (
                        &manifest_repo_id,
                        manifest_branch,
                        &edge.repo_name,
                        &edge.repo_branch,
                    )
                })
                .collect();
            let (txn_, _) = InsertEdges::query_with_transaction(txn, rows.as_slice())
                .await
                .with_context(|| {
                    format!(
                        "Failed to insert edges for manifest repo {manifest_repo_id} branch {manifest_branch}"
                    )
                })?;
            txn = txn_;
        }

        if let Some(log_id) = watermark {
            let (txn_, _) =
                SetWatermark::query_with_transaction(txn, &manifest_repo_id, &log_id)
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to set watermark for manifest repo {manifest_repo_id} while replacing membership"
                        )
                    })?;
            txn = txn_;
        }

        txn.commit().await.with_context(|| {
            format!(
                "Failed to commit membership replacement for manifest repo {manifest_repo_id} branch {manifest_branch}"
            )
        })?;
        Ok(())
    }

    async fn get_watermark(
        &self,
        ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        staleness: Staleness,
    ) -> Result<Option<i64>> {
        let rows = GetWatermark::query(
            self.get_connection(staleness),
            ctx.sql_query_telemetry(),
            &manifest_repo_id,
        )
        .await
        .with_context(|| {
            format!("Failure fetching watermark for manifest repo {manifest_repo_id}")
        })?;
        Ok(rows.into_iter().next().map(|(log_id,)| log_id))
    }

    async fn set_watermark(
        &self,
        ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        log_id: i64,
    ) -> Result<()> {
        SetWatermark::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            &manifest_repo_id,
            &log_id,
        )
        .await
        .with_context(|| {
            format!("Failed to set watermark for manifest repo {manifest_repo_id} to {log_id}")
        })?;
        Ok(())
    }
}
