/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use async_trait::async_trait;
use configmodel::Config;
use configmodel::ConfigExt;
use dag::DagAlgorithm;
use dag::Set;
use dag::Vertex;
use hg_metrics::increment_counter;
use manifest::Manifest;
use manifest_tree::TreeManifest;
use manifest_tree::TreeStore;
use pathhistory::PathHistory;
use pathmatcher::Matcher;
use storemodel::ReadRootTreeIds;
use types::HgId;
use types::RepoPath;
use types::RepoPathBuf;

use crate::error::CopyTraceError;
use crate::utils::compute_missing_files;
use crate::CopyTrace;
use crate::RenameFinder;
use crate::SearchDirection;
use crate::TraceResult;

/// limits the number of commits in path_copies
const DEFAULT_PATH_COPIES_COMMIT_LIMIT: u64 = 100;
/// limits the missing files we will check
const DEFAULT_MAX_MISSING_FILES: usize = 1000;

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

    // Read configs
    config: Arc<dyn Config + Send + Sync>,
}

impl DagCopyTrace {
    pub fn new(
        root_tree_reader: Arc<dyn ReadRootTreeIds + Send + Sync>,
        tree_store: Arc<dyn TreeStore>,
        rename_finder: Arc<dyn RenameFinder + Send + Sync>,
        dag: Arc<dyn DagAlgorithm + Send + Sync>,
        config: Arc<dyn Config + Send + Sync>,
    ) -> Result<Self> {
        let dag_copy_trace = Self {
            root_tree_reader,
            tree_store,
            rename_finder,
            dag,
            config,
        };
        Ok(dag_copy_trace)
    }

    async fn vertex_to_tree_manifest(&self, commit: &Vertex) -> Result<TreeManifest> {
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
        src: Vertex,
        dst: Vertex,
        path: RepoPathBuf,
    ) -> Result<Option<Vertex>> {
        let set = self.dag.range(src.into(), dst.into()).await?;
        let mut rename_tracer = PathHistory::new_existence_tracer(
            set,
            path,
            self.root_tree_reader.clone(),
            self.tree_store.clone(),
        )
        .await?;
        let rename_commit = rename_tracer.next().await?;
        Ok(rename_commit)
    }

    async fn find_rename_in_direction(
        &self,
        commit: Vertex,
        path: &RepoPath,
        direction: SearchDirection,
    ) -> Result<(Option<RepoPathBuf>, Vertex)> {
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

    async fn check_path(&self, target_commit: &Vertex, path: RepoPathBuf) -> Result<TraceResult> {
        let tree = self.vertex_to_tree_manifest(target_commit).await?;
        if tree.get(&path)?.is_some() {
            Ok(TraceResult::Renamed(path))
        } else {
            Ok(TraceResult::NotFound)
        }
    }

    async fn compute_distance(&self, src: Vertex, dst: Vertex) -> Result<u64> {
        let src: Set = src.into();
        let dst: Set = dst.into();
        let distance = self
            .dag
            .only(src.clone(), dst.clone())
            .await?
            .count()
            .await?
            + self.dag.only(dst, src).await?.count().await?;
        Ok(distance)
    }
}

#[async_trait]
impl CopyTrace for DagCopyTrace {
    async fn trace_rename(
        &self,
        src: Vertex,
        dst: Vertex,
        src_path: RepoPathBuf,
    ) -> Result<TraceResult> {
        tracing::debug!(?src, ?dst, ?src_path, "trace_reanme");

        let msrc = self.vertex_to_tree_manifest(&src).await?;
        if msrc.get(&src_path)?.is_none() {
            tracing::debug!("src_path not found in src commit");
            return Ok(TraceResult::NotFound);
        }

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
        src: Vertex,
        dst: Vertex,
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
                None => {
                    tracing::trace!(" no rename commit found");
                    return self.check_path(&target, curr_path).await;
                }
            };
            tracing::trace!(?rename_commit, " found");

            if rename_commit == target {
                return Ok(TraceResult::Renamed(curr_path));
            }
            let (next_path, next_commit) = self
                .find_rename_in_direction(
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
        src: Vertex,
        dst: Vertex,
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
                None => {
                    tracing::trace!(" no rename commit found");
                    return self.check_path(&target, curr_path).await;
                }
            };
            tracing::trace!(?rename_commit, " found");

            if rename_commit == curr {
                return Ok(TraceResult::Renamed(curr_path));
            }
            let (next_path, next_commit) = self
                .find_rename_in_direction(
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

    /// find {x@dst: y@src} copy mapping for directed compare
    ///
    /// Assuming m = len(added_files) and n = len(deleted_files), then the time complexity is:
    ///   1. if src and dst are adjacent:
    ///     * Sapling backend: O(added files)
    ///     * Git backend: O(m * min(n, max_rename_candidates))
    ///   2. else:
    ///     * O(min(m, max_missing_files) * min(n, max_rename_candidates) * rename_times)
    ///         * rename_times should be a small number (usually 1)
    async fn path_copies(
        &self,
        src: Vertex,
        dst: Vertex,
        matcher: Option<Arc<dyn Matcher + Send + Sync>>,
    ) -> Result<HashMap<RepoPathBuf, RepoPathBuf>> {
        tracing::trace!(?src, ?dst, "path_copies");

        let start_time = Instant::now();
        let msrc = self.vertex_to_tree_manifest(&src).await?;
        let mdst = self.vertex_to_tree_manifest(&dst).await?;

        // 1. src and dst are adjacent

        let dst_parents = self.dag.parent_names(dst.clone()).await?;
        for parent in dst_parents {
            if parent == src {
                let copies = self.rename_finder.find_renames(&msrc, &mdst, matcher).await;
                tracing::debug!(
                    duration = start_time.elapsed().as_millis() as u64,
                    "path_copies - src is parent of dst"
                );
                return copies;
            }
        }
        let src_parents = self.dag.parent_names(src.clone()).await?;
        for parent in src_parents {
            if parent == dst {
                let mut copies = self
                    .rename_finder
                    .find_renames(&mdst, &msrc, matcher)
                    .await?
                    .into_iter()
                    .collect::<Vec<_>>();
                copies.sort_unstable();
                let reverse_copies = copies.into_iter().map(|(k, v)| (v, k)).collect();
                tracing::debug!(
                    duration = start_time.elapsed().as_millis() as u64,
                    "path_copies - dst is parent of src"
                );
                return Ok(reverse_copies);
            }
        }

        // 2. src and dst are not adjacent

        let mut result = HashMap::new();
        let distance = self.compute_distance(src.clone(), dst.clone()).await?;
        let max_commit_limit = get_path_copies_commit_limit(&self.config)?;
        tracing::trace!(?distance, ?max_commit_limit, "distance between src and dst");
        if distance > max_commit_limit {
            // skip calculating path copies if too many commits
            tracing::debug!(
                duration = start_time.elapsed().as_millis() as u64,
                "path_copies - distance > max_commit_limit"
            );
            return Ok(result);
        }

        let find_count_limit = get_max_missing_files(&self.config)?;
        let missing = compute_missing_files(&msrc, &mdst, matcher, Some(find_count_limit))?;
        tracing::trace!(missing_len = missing.len(), "missing files");
        for dst_path in missing {
            let src_path = self
                .trace_rename(dst.clone(), src.clone(), dst_path.clone())
                .await?;
            if let TraceResult::Renamed(src_path) = src_path {
                result.insert(dst_path, src_path);
            }
        }

        tracing::debug!(
            duration = start_time.elapsed().as_millis() as u64,
            "path_copies - src and dst are not adjacent"
        );
        Ok(result)
    }
}

fn get_path_copies_commit_limit(config: &dyn Config) -> Result<u64> {
    let v = config
        .get_opt::<u64>("copytrace", "pathcopiescommitlimit")?
        .unwrap_or(DEFAULT_PATH_COPIES_COMMIT_LIMIT);
    Ok(v)
}

fn get_max_missing_files(config: &dyn Config) -> Result<usize> {
    let v = config
        .get_opt::<usize>("copytrace", "maxmissingfiles")?
        .unwrap_or(DEFAULT_MAX_MISSING_FILES);
    Ok(v)
}
