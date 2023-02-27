/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use dag::Vertex;
use types::RepoPathBuf;

/// Tracing the rename history of a file for rename detection in rebase, amend etc
pub trait CopyTrace {
    /// Trace the corresponding path of `src_path` in `dst` vertex across renames.
    /// Depending on the relationship of `src` and `dst`, it will search backward,
    /// forward or both.
    fn trace_rename(src: Vertex, dst: Vertex, src_path: RepoPathBuf) -> Option<RepoPathBuf>;

    /// Trace the corresponding path of `dst_path` in `src` commit across renames.
    /// It will search backward, i.e. from `dst` to `src` vertex.
    fn trace_rename_backward(
        src: Vertex,
        dst: Vertex,
        dst_path: RepoPathBuf,
    ) -> Option<RepoPathBuf>;

    /// Trace the corresponding path of `src_path` in `dst` commit across renames.
    /// It will search forward, i.e. from `src` to `dst` vertex.
    fn trace_rename_forward(src: Vertex, dst: Vertex, src_path: RepoPathBuf)
    -> Option<RepoPathBuf>;

    /// Find rename file paris in the specified `commit`.
    fn find_renames(commit: Vertex) -> HashMap<RepoPathBuf, RepoPathBuf>;
}
