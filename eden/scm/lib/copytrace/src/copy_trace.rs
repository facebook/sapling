/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use dag::Vertex;
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
    /// - the source given source file is not in the source commit
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
