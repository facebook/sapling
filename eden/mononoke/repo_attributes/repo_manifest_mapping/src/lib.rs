/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! A dedicated SQL store for the repo-manifest membership mapping.
//!
//! Maintains a bidirectional `(repo_name, repo_branch) -> [(manifest_repo_id,
//! manifest_branch)]` membership projection, plus a per-manifest-branch tailer
//! watermark.
//!
//! Here "manifest" means an AOSP/west-style repo-manifest (a `default.xml`
//! listing member repos and their branches), NOT a Mononoke derived-data
//! manifest. Keys are git ref names and are treated as CASE-SENSITIVE raw
//! bytes. `manifest_repo_id` scopes each row to the manifest repo that owns the
//! manifest branch, so multiple manifest repos (e.g. AOSP and a west/Zephyr
//! firmware manifest) can coexist.
//!
//! This is a GLOBAL/shared store (modeled on `git_source_of_truth`): it is not
//! scoped to a single Mononoke repo, so the store holds only its connections
//! and callers pass a `CoreContext` per call.
//!
//! The `#[facet::facet]` attribute is kept only for its `Arc`/`Ref` DI aliases;
//! because every consumer (the backfill/tailer/reconciler jobs and the
//! land-service read API) is cross-repo, the store is constructed directly and
//! passed around as `Arc<dyn RepoManifestMapping>` rather than composed onto the
//! per-repo `Repo` container. If an in-server per-repo consumer ever needs it,
//! wrap it in a per-repo facet (à la `RepoCrossRepo`) rather than registering
//! this global store on `Repo` directly.

mod store;
mod types;

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::RepositoryId;

pub use crate::store::SqlRepoManifestMapping;
pub use crate::store::SqlRepoManifestMappingBuilder;
pub use crate::types::ManifestBranch;
pub use crate::types::MembershipEdge;
pub use crate::types::RepoBranch;
pub use crate::types::RepoName;

/// Requested staleness for a read against the mapping store.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Staleness {
    /// The most recent state (served by the write master).
    MostRecent,
    /// Best-effort recency (may be served by a replica).
    MaybeStale,
}

#[facet::facet]
#[async_trait]
/// Bidirectional repo-manifest membership projection.
///
/// "manifest" refers to an AOSP/west-style repo-manifest (`default.xml`), not a
/// Mononoke derived-data manifest.
pub trait RepoManifestMapping: Send + Sync {
    /// Reverse (hot) fan-out read: every `(manifest_repo_id, manifest_branch)`
    /// that includes the given member repo on the given branch, across all
    /// manifest repos. Results are already DISTINCT.
    async fn manifest_branches_for_repo(
        &self,
        ctx: &CoreContext,
        repo_name: &RepoName,
        repo_branch: &RepoBranch,
        staleness: Staleness,
    ) -> Result<Vec<(RepositoryId, ManifestBranch)>>;

    /// Forward read: all member repos of the given manifest branch, scoped to
    /// the owning manifest repo.
    async fn members_for_manifest_branch(
        &self,
        ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        manifest_branch: &ManifestBranch,
        staleness: Staleness,
    ) -> Result<Vec<MembershipEdge>>;

    /// Atomically replace the entire membership set of a manifest branch,
    /// optionally advancing that manifest branch's tailer watermark in the SAME
    /// transaction.
    ///
    /// Runs a delete-then-bulk-insert (+ optional watermark set) as one
    /// transaction. The `edges` batch is treated as a set — duplicate edges are
    /// de-duplicated, not rejected (a manifest may list the same repo+branch via
    /// multiple paths). Idempotent under replay.
    async fn replace_membership(
        &self,
        ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        manifest_branch: &ManifestBranch,
        edges: &[MembershipEdge],
        watermark: Option<i64>,
    ) -> Result<()>;

    /// Read the tailer watermark for one manifest branch (`None` if never set).
    async fn get_branch_watermark(
        &self,
        ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        manifest_branch: &ManifestBranch,
        staleness: Staleness,
    ) -> Result<Option<i64>>;

    /// The read cursor for a manifest repo: the largest per-branch watermark
    /// (`None` if the repo has no branches yet). The tailer reads new log entries
    /// from here so it always advances (a dormant branch can't pin it).
    async fn get_read_cursor(
        &self,
        ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        staleness: Staleness,
    ) -> Result<Option<i64>>;

    /// Set (upsert) the tailer watermark for one manifest branch.
    async fn set_branch_watermark(
        &self,
        ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        manifest_branch: &ManifestBranch,
        log_id: i64,
    ) -> Result<()>;
}

/// A no-op double that stores nothing and reports empty/ok everywhere, for
/// contexts that must supply a `RepoManifestMapping` but do not use it.
#[derive(Clone)]
pub struct NoopRepoManifestMapping {}

#[async_trait]
impl RepoManifestMapping for NoopRepoManifestMapping {
    async fn manifest_branches_for_repo(
        &self,
        _ctx: &CoreContext,
        _repo_name: &RepoName,
        _repo_branch: &RepoBranch,
        _staleness: Staleness,
    ) -> Result<Vec<(RepositoryId, ManifestBranch)>> {
        Ok(vec![])
    }

    async fn members_for_manifest_branch(
        &self,
        _ctx: &CoreContext,
        _manifest_repo_id: RepositoryId,
        _manifest_branch: &ManifestBranch,
        _staleness: Staleness,
    ) -> Result<Vec<MembershipEdge>> {
        Ok(vec![])
    }

    async fn replace_membership(
        &self,
        _ctx: &CoreContext,
        _manifest_repo_id: RepositoryId,
        _manifest_branch: &ManifestBranch,
        _edges: &[MembershipEdge],
        _watermark: Option<i64>,
    ) -> Result<()> {
        Ok(())
    }

    async fn get_branch_watermark(
        &self,
        _ctx: &CoreContext,
        _manifest_repo_id: RepositoryId,
        _manifest_branch: &ManifestBranch,
        _staleness: Staleness,
    ) -> Result<Option<i64>> {
        Ok(None)
    }

    async fn get_read_cursor(
        &self,
        _ctx: &CoreContext,
        _manifest_repo_id: RepositoryId,
        _staleness: Staleness,
    ) -> Result<Option<i64>> {
        Ok(None)
    }

    async fn set_branch_watermark(
        &self,
        _ctx: &CoreContext,
        _manifest_repo_id: RepositoryId,
        _manifest_branch: &ManifestBranch,
        _log_id: i64,
    ) -> Result<()> {
        Ok(())
    }
}

/// A full stored row: the member repo edge together with its owning
/// `(manifest_repo_id, manifest_branch)` context.
type StoredRow = (RepositoryId, ManifestBranch, RepoName, RepoBranch);

#[derive(Default)]
struct TestState {
    rows: HashSet<StoredRow>,
    watermarks: HashMap<(RepositoryId, ManifestBranch), i64>,
}

/// An in-memory double that mirrors the SQL store's observable semantics
/// (reverse-read dedup, replace-then-insert, manifest-repo scoping,
/// case-sensitive keys) without a database. Cheap to construct and share.
#[derive(Clone)]
pub struct TestRepoManifestMapping {
    state: Arc<Mutex<TestState>>,
}

impl TestRepoManifestMapping {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(TestState::default())),
        }
    }
}

impl Default for TestRepoManifestMapping {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RepoManifestMapping for TestRepoManifestMapping {
    async fn manifest_branches_for_repo(
        &self,
        _ctx: &CoreContext,
        repo_name: &RepoName,
        repo_branch: &RepoBranch,
        _staleness: Staleness,
    ) -> Result<Vec<(RepositoryId, ManifestBranch)>> {
        let state = self.state.lock().expect("poisoned lock");
        let mut result: Vec<(RepositoryId, ManifestBranch)> = state
            .rows
            .iter()
            .filter(|(_, _, rn, rb)| rn == repo_name && rb == repo_branch)
            .map(|(repo_id, manifest_branch, _, _)| (*repo_id, manifest_branch.clone()))
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();
        result.sort();
        Ok(result)
    }

    async fn members_for_manifest_branch(
        &self,
        _ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        manifest_branch: &ManifestBranch,
        _staleness: Staleness,
    ) -> Result<Vec<MembershipEdge>> {
        let state = self.state.lock().expect("poisoned lock");
        let mut result: Vec<MembershipEdge> = state
            .rows
            .iter()
            .filter(|(repo_id, mb, _, _)| *repo_id == manifest_repo_id && mb == manifest_branch)
            .map(|(_, _, rn, rb)| MembershipEdge::new(rn.clone(), rb.clone()))
            .collect();
        result.sort();
        Ok(result)
    }

    async fn replace_membership(
        &self,
        _ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        manifest_branch: &ManifestBranch,
        edges: &[MembershipEdge],
        watermark: Option<i64>,
    ) -> Result<()> {
        let mut state = self.state.lock().expect("poisoned lock");
        state
            .rows
            .retain(|(repo_id, mb, _, _)| !(*repo_id == manifest_repo_id && mb == manifest_branch));
        for edge in edges {
            state.rows.insert((
                manifest_repo_id,
                manifest_branch.clone(),
                edge.repo_name.clone(),
                edge.repo_branch.clone(),
            ));
        }
        if let Some(log_id) = watermark {
            state
                .watermarks
                .insert((manifest_repo_id, manifest_branch.clone()), log_id);
        }
        Ok(())
    }

    async fn get_branch_watermark(
        &self,
        _ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        manifest_branch: &ManifestBranch,
        _staleness: Staleness,
    ) -> Result<Option<i64>> {
        let state = self.state.lock().expect("poisoned lock");
        Ok(state
            .watermarks
            .get(&(manifest_repo_id, manifest_branch.clone()))
            .copied())
    }

    async fn get_read_cursor(
        &self,
        _ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        _staleness: Staleness,
    ) -> Result<Option<i64>> {
        let state = self.state.lock().expect("poisoned lock");
        Ok(state
            .watermarks
            .iter()
            .filter(|((repo_id, _), _)| *repo_id == manifest_repo_id)
            .map(|(_, log_id)| *log_id)
            .max())
    }

    async fn set_branch_watermark(
        &self,
        _ctx: &CoreContext,
        manifest_repo_id: RepositoryId,
        manifest_branch: &ManifestBranch,
        log_id: i64,
    ) -> Result<()> {
        let mut state = self.state.lock().expect("poisoned lock");
        state
            .watermarks
            .insert((manifest_repo_id, manifest_branch.clone()), log_id);
        Ok(())
    }
}
