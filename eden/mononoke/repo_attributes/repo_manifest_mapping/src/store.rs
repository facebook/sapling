/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use futures_retry::retry;
use metaconfig_types::MetadataDatabaseConfig;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::Connection;
use sql_ext::SqlConnections;
use sql_ext::mononoke_queries;
use sql_ext::should_retry_query;

use crate::RepoManifestMapping;
use crate::Staleness;
use crate::types::ManifestBranch;
use crate::types::MembershipEdge;
use crate::types::RepoBranch;
use crate::types::RepoName;

/// Rows per bulk-INSERT batch — keeps bound params under the SQLite/MySQL limit.
const INSERT_CHUNK_SIZE: usize = 1000;
/// Backstop for the residual deadlocks that insert ordering cannot rule out.
const MAX_REPLACE_ATTEMPTS: usize = 5;
const REPLACE_RETRY_BASE_INTERVAL: Duration = Duration::from_millis(100);
const REPLACE_RETRY_JITTER: Duration = Duration::from_millis(200);

mononoke_queries! {
    // Hot reverse / fan-out read: which manifest branches (across all manifest
    // repos) include the given member repo on the given repo branch. ORDER BY
    // makes the output sequence a contract rather than an incidental index-scan
    // artifact, so the in-memory Test double (which sorts) is an
    // observationally faithful mirror.
    // No DISTINCT: the 4-column PK forbids duplicates, and it would cost a temp table.
    read GetManifestBranchesForRepo(
        repo_name: RepoName,
        repo_branch: RepoBranch,
    ) -> (RepositoryId, ManifestBranch) {
        "SELECT manifest_repo_id, manifest_branch
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
    // before inserting, so the VALUES list is always primary-key-clean.
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

    // Per-branch watermark: the last processed log id for one manifest branch.
    read GetBranchWatermark(repo_id: RepositoryId, manifest_branch: ManifestBranch) -> (i64,) {
        "SELECT log_id FROM manifest_watermark WHERE repo_id = {repo_id} AND manifest_branch = {manifest_branch}"
    }

    // Read cursor for a manifest repo: the LARGEST per-branch watermark, i.e. the
    // highest log id applied for any branch. The tailer reads new entries from
    // here so it always advances — a dormant branch's stale watermark can't pin
    // it. Per-branch watermarks remain the source of truth; if a future
    // per-repo-per-bookmark model breaks per-repo id monotonicity, only this read
    // strategy changes (e.g. per-branch reads), not the schema. Returns 0 rows
    // when the repo has no branches yet.
    read GetReadCursor(repo_id: RepositoryId) -> (i64,) {
        "SELECT log_id FROM manifest_watermark WHERE repo_id = {repo_id} ORDER BY log_id DESC LIMIT 1"
    }

    // Every manifest branch the tailer has seen. Read from the watermark table, not
    // the edge table: one row per branch there versus hundreds of thousands here.
    read ListManifestBranches(repo_id: RepositoryId) -> (ManifestBranch,) {
        "SELECT manifest_branch FROM manifest_watermark WHERE repo_id = {repo_id}"
    }

    // Unconditional per-branch upsert, deliberately NOT a compare-and-swap.
    // Exactly-once comes from advancing the branch watermark in the SAME
    // transaction as the membership replace (see `replace_membership`), and the
    // owning tailer is a single-leader singleton. Add a CAS guard only if a
    // future concurrent writer needs it.
    write SetBranchWatermark(repo_id: RepositoryId, manifest_branch: ManifestBranch, log_id: i64) {
        none,
        "REPLACE INTO manifest_watermark (repo_id, manifest_branch, log_id) VALUES ({repo_id}, {manifest_branch}, {log_id})"
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

// Binds the dedicated `xdb.mononoke_manifest` tier. Fails loud when unconfigured
// rather than falling back to the shared metadata DB, so routing data never lands there.
impl SqlConstructFromMetadataDatabaseConfig for SqlRepoManifestMappingBuilder {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&RemoteDatabaseConfig> {
        remote.repo_manifest_mapping.as_ref()
    }
    fn oss_remote_database_config(
        remote: &OssRemoteMetadataDatabaseConfig,
    ) -> Option<&OssRemoteDatabaseConfig> {
        Some(&remote.production)
    }
}

/// Whether `metadata` declares the routing tier. Pure config inspection, so a
/// caller can skip building a store without opening a connection.
pub fn is_configured(metadata: &MetadataDatabaseConfig) -> bool {
    match metadata {
        MetadataDatabaseConfig::Local(_) => true,
        MetadataDatabaseConfig::Remote(remote) => remote.repo_manifest_mapping.is_some(),
        MetadataDatabaseConfig::OssRemote(_) => true,
    }
}

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
        // INSERT free of primary-key conflicts and matches set semantics; the
        // in-memory Test double dedups identically.
        let mut seen = std::collections::HashSet::with_capacity(edges.len());
        let mut deduped: Vec<&MembershipEdge> = edges.iter().filter(|e| seen.insert(*e)).collect();

        // Deterministic key order: concurrent replacements must take index locks in the same sequence or deadlock.
        deduped.sort_unstable_by(|a, b| {
            (&a.repo_name, &a.repo_branch).cmp(&(&b.repo_name, &b.repo_branch))
        });

        // Chunk to stay under the bind-variable limit; empty edges yield no chunks
        // (a legitimate clear), avoiding an invalid empty VALUES clause.
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

        // Backstop for what ordering can't prevent; delete-then-insert makes each attempt idempotent.
        retry(
            |_| async {
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

                for chunk in rows.chunks(INSERT_CHUNK_SIZE) {
                    let (txn_, _) = InsertEdges::query_with_transaction(txn, chunk)
                        .await
                        .with_context(|| {
                            format!(
                                "Failed to insert edges for manifest repo {manifest_repo_id} branch {manifest_branch}"
                            )
                        })?;
                    txn = txn_;
                }

                if let Some(log_id) = watermark {
                    let (txn_, _) = SetBranchWatermark::query_with_transaction(
                        txn,
                        &manifest_repo_id,
                        manifest_branch,
                        &log_id,
                    )
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to set watermark for manifest repo {manifest_repo_id} branch {manifest_branch} while replacing membership"
                        )
                    })?;
                    txn = txn_;
                }

                txn.commit().await.with_context(|| {
                    format!(
                        "Failed to commit membership replacement for manifest repo {manifest_repo_id} branch {manifest_branch}"
                    )
                })?;
                anyhow::Ok(())
            },
            REPLACE_RETRY_BASE_INTERVAL,
        )
        .exponential_backoff(1.2)
        .jitter(REPLACE_RETRY_JITTER)
        .retry_if(|_attempt, err| should_retry_query(err))
        .max_attempts(MAX_REPLACE_ATTEMPTS)
        .await?;

        Ok(())
    }

    async fn get_branch_watermark(
        &self,
        ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        manifest_branch: &ManifestBranch,
        staleness: Staleness,
    ) -> Result<Option<i64>> {
        let rows = GetBranchWatermark::query(
            self.get_connection(staleness),
            ctx.sql_query_telemetry(),
            &manifest_repo_id,
            manifest_branch,
        )
        .await
        .with_context(|| {
            format!(
                "Failure fetching watermark for manifest repo {manifest_repo_id} branch {manifest_branch}"
            )
        })?;
        Ok(rows.into_iter().next().map(|(log_id,)| log_id))
    }

    async fn get_read_cursor(
        &self,
        ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        staleness: Staleness,
    ) -> Result<Option<i64>> {
        let rows = GetReadCursor::query(
            self.get_connection(staleness),
            ctx.sql_query_telemetry(),
            &manifest_repo_id,
        )
        .await
        .with_context(|| {
            format!("Failure fetching read cursor for manifest repo {manifest_repo_id}")
        })?;
        Ok(rows.into_iter().next().map(|(log_id,)| log_id))
    }

    async fn list_manifest_branches(
        &self,
        ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        staleness: Staleness,
    ) -> Result<Vec<ManifestBranch>> {
        let rows = ListManifestBranches::query(
            self.get_connection(staleness),
            ctx.sql_query_telemetry(),
            &manifest_repo_id,
        )
        .await
        .with_context(|| {
            format!("Failure listing manifest branches for manifest repo {manifest_repo_id}")
        })?;
        Ok(rows.into_iter().map(|(branch,)| branch).collect())
    }

    async fn set_branch_watermark(
        &self,
        ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        manifest_branch: &ManifestBranch,
        log_id: i64,
    ) -> Result<()> {
        SetBranchWatermark::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            &manifest_repo_id,
            manifest_branch,
            &log_id,
        )
        .await
        .with_context(|| {
            format!(
                "Failed to set watermark for manifest repo {manifest_repo_id} branch {manifest_branch} to {log_id}"
            )
        })?;
        Ok(())
    }
}
