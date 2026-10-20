/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dag::Vertex;
use hg_metrics::increment_counter;
use pathmatcher::Matcher;
use serde::Serialize;
use types::RepoPathBuf;

/// Tracing Result of CopyTrace's trace_XXX method.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "t", content = "c")]
pub enum TraceResult {
    /// Found the renamed-to path of the given source file, return it.
    Renamed(RepoPathBuf),
    /// The file was deleted by a commit between common ancestor and destination commits.
    Deleted(Vertex, RepoPathBuf),
    /// The file was added by a commit between common ancestor and source commits.
    Added(Vertex, RepoPathBuf),
    /// Did not find the renamed-to path and the deletion commit, for example:
    /// - there is no common ancestor between source and destination commits
    /// - the given source file is not in the source commit
    NotFound,
}

/// Tracing the rename history of a file for rename detection in rebase, amend etc
#[async_trait]
pub trait CopyTrace {
    /// Trace the corresponding path of `src_path` in `dst` vertex across renames.
    /// Depending on the relationship of `src` and `dst`, it will search backward,
    /// forward or both.
    async fn trace_rename(
        &self,
        src: Vertex,
        dst: Vertex,
        src_path: RepoPathBuf,
    ) -> Result<TraceResult>;

    /// Trace the corresponding path of `dst_path` in `src` commit across renames.
    /// It will search backward, i.e. from `dst` to `src` vertex.
    async fn trace_rename_backward(
        &self,
        src: Vertex,
        dst: Vertex,
        dst_path: RepoPathBuf,
    ) -> Result<TraceResult>;

    /// Trace the corresponding path of `src_path` in `dst` commit across renames.
    /// It will search forward, i.e. from `src` to `dst` vertex.
    async fn trace_rename_forward(
        &self,
        src: Vertex,
        dst: Vertex,
        src_path: RepoPathBuf,
    ) -> Result<TraceResult>;

    /// find {x@dst: y@src} copy mapping for directed compare
    async fn path_copies(
        &self,
        src: Vertex,
        dst: Vertex,
        matcher: Option<Arc<dyn Matcher + Send + Sync>>,
    ) -> Result<HashMap<RepoPathBuf, RepoPathBuf>>;
}

/// Trace each path in `src_paths` from `src` to `dst`.
///
/// Returns a `{dst_path: src_path}` map of the paths with a `TraceResult::Renamed` result.
/// If the trace of a path fails, this function logs the error, increments the
/// `copytrace_trace_rename_error` counter, and skips the path.
pub async fn trace_renames(
    copy_trace: &(dyn CopyTrace + Send + Sync),
    src: Vertex,
    dst: Vertex,
    src_paths: Vec<RepoPathBuf>,
) -> HashMap<RepoPathBuf, RepoPathBuf> {
    let mut renames = HashMap::new();
    for src_path in src_paths {
        match copy_trace
            .trace_rename(src.clone(), dst.clone(), src_path.clone())
            .await
        {
            Ok(TraceResult::Renamed(dst_path)) => {
                renames.insert(dst_path, src_path);
            }
            Ok(_) => {}
            Err(err) => {
                tracing::warn!(?src, ?dst, ?src_path, ?err, "failed to trace rename");
                increment_counter("copytrace_trace_rename_error", 1);
            }
        }
    }
    renames
}
