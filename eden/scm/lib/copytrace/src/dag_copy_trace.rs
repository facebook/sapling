/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dag::DagAlgorithm;
use hg_metrics::increment_counter;
use manifest::Manifest;
use manifest_tree::TreeManifest;
use manifest_tree::TreeStore;
use pathhistory::RenameTracer;
use storemodel::ReadRootTreeIds;
use types::HgId;
use types::RepoPath;
use types::RepoPathBuf;

use crate::error::CopyTraceError;
use crate::CopyTrace;
use crate::RenameFinder;
use crate::SearchDirection;
use crate::TraceResult;

pub struct DagCopyTrace {
    /* Input */
    /// Resolve commit ids to trees in batch.
    root_tree_reader: Arc<dyn ReadRootTreeIds + Send + Sync>,

    /// Resolve and prefetch trees in batch.
    tree_store: Arc<dyn TreeStore>,

    // Find renames for given commits
    rename_finder: Arc<dyn RenameFinder + Send + Sync>,

    /// Commit graph algorithms
    dag: Arc<dyn DagAlgorithm + Send + Sync>,
}

impl DagCopyTrace {
    pub fn new(
        root_tree_reader: Arc<dyn ReadRootTreeIds + Send + Sync>,
        tree_store: Arc<dyn TreeStore>,
        rename_finder: Arc<dyn RenameFinder + Send + Sync>,
        dag: Arc<dyn DagAlgorithm + Send + Sync>,
    ) -> Result<Self> {
        let dag_copy_trace = Self {
            root_tree_reader,
            tree_store,
            rename_finder,
            dag,
        };
        Ok(dag_copy_trace)
    }

    async fn vertex_to_tree_manifest(&self, commit: &dag::Vertex) -> Result<TreeManifest> {
        let commit_id = HgId::from_slice(commit.as_ref())?;
        let commit_to_tree_id = self
            .root_tree_reader
            .read_root_tree_ids(vec![commit_id])
            .await?;
        if commit_to_tree_id.is_empty() {
            return Err(CopyTraceError::RootTreeIdNotFound(commit_id).into());
        }
        let (_, tree_id) = commit_to_tree_id[0];
        Ok(TreeManifest::durable(self.tree_store.clone(), tree_id))
    }

    async fn trace_rename_commit(
        &self,
        src: dag::Vertex,
        dst: dag::Vertex,
        path: RepoPathBuf,
    ) -> Result<Option<dag::Vertex>> {
        let set = self.dag.range(src.into(), dst.into()).await?;
        let mut rename_tracer = RenameTracer::new(
            set,
            path,
            self.root_tree_reader.clone(),
            self.tree_store.clone(),
        )
        .await?;
        let rename_commit = rename_tracer.next().await?;
        Ok(rename_commit)
    }

    async fn find_renames_in_direction(
        &self,
        commit: dag::Vertex,
        path: &RepoPath,
        direction: SearchDirection,
    ) -> Result<(Option<RepoPathBuf>, dag::Vertex)> {
        let parents = self.dag.parent_names(commit.clone()).await?;
        if parents.is_empty() {
            return Err(CopyTraceError::NoParents(commit).into());
        }
        // For simplicity, we only check p1.
        let p1 = &parents[0];
        let old_manifest = self.vertex_to_tree_manifest(p1).await?;
        let new_manifest = self.vertex_to_tree_manifest(&commit).await?;
        let (rename, next_commit) = match direction {
            SearchDirection::Backward => {
                let rename = self
                    .rename_finder
                    .find_rename_backward(&old_manifest, &new_manifest, path, &commit)
                    .await?;
                (rename, p1.clone())
            }
            SearchDirection::Forward => {
                let rename = self
                    .rename_finder
                    .find_rename_forward(&old_manifest, &new_manifest, path, &commit)
                    .await?;
                (rename, commit)
            }
        };
        Ok((rename, next_commit))
    }

    async fn check_path(
        &self,
        target_commit: &dag::Vertex,
        path: RepoPathBuf,
    ) -> Result<TraceResult> {
        let tree = self.vertex_to_tree_manifest(target_commit).await?;
        if tree.get(&path)?.is_some() {
            Ok(TraceResult::Renamed(path))
        } else {
            Ok(TraceResult::NotFound)
        }
    }
}

#[async_trait]
impl CopyTrace for DagCopyTrace {
    async fn trace_rename(
        &self,
        src: dag::Vertex,
        dst: dag::Vertex,
        src_path: RepoPathBuf,
    ) -> Result<TraceResult> {
        tracing::debug!(?src, ?dst, ?src_path, "trace_reanme");
        if self.dag.is_ancestor(src.clone(), dst.clone()).await? {
            return self
                .trace_rename_forward(src.clone(), dst.clone(), src_path)
                .await;
        } else if self.dag.is_ancestor(dst.clone(), src.clone()).await? {
            return self
                .trace_rename_backward(dst.clone(), src.clone(), src_path)
                .await;
        } else {
            let set = dag::Set::from_static_names(vec![src.clone(), dst.clone()]);
            let base = match self.dag.gca_one(set).await? {
                Some(base) => base,
                None => {
                    tracing::trace!("no common base");
                    increment_counter("copytrace_noCommonBase", 1);
                    return Ok(TraceResult::NotFound);
                }
            };
            tracing::trace!(?base);
            let base_result = self
                .trace_rename_backward(base.clone(), src, src_path)
                .await?;
            tracing::trace!(?base_result);
            match base_result {
                TraceResult::Renamed(base_path) => {
                    self.trace_rename_forward(base, dst, base_path).await
                }
                TraceResult::Added(_, _) => {
                    increment_counter("copytrace_notInCommonBase", 1);
                    Ok(base_result)
                }
                _ => Ok(base_result),
            }
        }
    }

    async fn trace_rename_backward(
        &self,
        src: dag::Vertex,
        dst: dag::Vertex,
        dst_path: RepoPathBuf,
    ) -> Result<TraceResult> {
        tracing::trace!(?src, ?dst, ?dst_path, "trace_rename_backward");
        let (mut curr, target, mut curr_path) = (dst, src, dst_path);

        loop {
            tracing::trace!(?curr, ?curr_path, " loop starts");
            let rename_commit = match self
                .trace_rename_commit(target.clone(), curr.clone(), curr_path.clone())
                .await?
            {
                Some(rename_commit) => rename_commit,
                None => return self.check_path(&target, curr_path).await,
            };
            tracing::trace!(?rename_commit, " found");

            if rename_commit == target {
                return Ok(TraceResult::Renamed(curr_path));
            }
            let (next_path, next_commit) = self
                .find_renames_in_direction(
                    rename_commit.clone(),
                    curr_path.as_repo_path(),
                    SearchDirection::Backward,
                )
                .await?;
            if let Some(next_path) = next_path {
                curr = next_commit;
                curr_path = next_path;
            } else {
                // no rename info for curr_path
                return Ok(TraceResult::Added(rename_commit, curr_path));
            }
        }
    }

    async fn trace_rename_forward(
        &self,
        src: dag::Vertex,
        dst: dag::Vertex,
        src_path: RepoPathBuf,
    ) -> Result<TraceResult> {
        tracing::trace!(?src, ?dst, ?src_path, "trace_rename_forward");
        let (mut curr, target, mut curr_path) = (src, dst, src_path);

        loop {
            tracing::trace!(?curr, ?curr_path, " loop starts");
            let rename_commit = match self
                .trace_rename_commit(curr.clone(), target.clone(), curr_path.clone())
                .await?
            {
                Some(rename_commit) => rename_commit,
                None => return self.check_path(&target, curr_path).await,
            };
            tracing::trace!(?rename_commit, " found");

            if rename_commit == curr {
                return Ok(TraceResult::Renamed(curr_path));
            }
            let (next_path, next_commit) = self
                .find_renames_in_direction(
                    rename_commit.clone(),
                    curr_path.as_repo_path(),
                    SearchDirection::Forward,
                )
                .await?;
            if let Some(next_path) = next_path {
                curr = next_commit;
                curr_path = next_path;
            } else {
                // no rename info for curr_path
                return Ok(TraceResult::Deleted(rename_commit, curr_path));
            }
        }
    }
}
