/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use dag::Vertex;
use types::RepoPathBuf;

/// Tracing Result of CopyTrace's trace_XXX method.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub enum TraceResult {
    /// Found the renamed-to path of the given source file, return it.
    Renamed(RepoPathBuf),
    /// Did not find the target path, return the commit that deleted the given
    /// source file.
    Deleted(Vertex),
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
    ) -> Result<Option<RepoPathBuf>>;

    /// Trace the corresponding path of `dst_path` in `src` commit across renames.
    /// It will search backward, i.e. from `dst` to `src` vertex.
    async fn trace_rename_backward(
        &self,
        src: Vertex,
        dst: Vertex,
        dst_path: RepoPathBuf,
    ) -> Result<Option<RepoPathBuf>>;

    /// Trace the corresponding path of `src_path` in `dst` commit across renames.
    /// It will search forward, i.e. from `src` to `dst` vertex.
    async fn trace_rename_forward(
        &self,
        src: Vertex,
        dst: Vertex,
        src_path: RepoPathBuf,
    ) -> Result<Option<RepoPathBuf>>;
}
