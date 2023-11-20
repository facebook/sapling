/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod copy_trace;
mod dag_copy_trace;
mod error;
mod git_copy_trace;
mod rename_finders;
mod utils;

pub use crate::copy_trace::CopyTrace;
pub use crate::copy_trace::TraceResult;
pub use crate::dag_copy_trace::DagCopyTrace;
pub use crate::git_copy_trace::GitCopyTrace;
pub use crate::rename_finders::ContentSimilarityRenameFinder;
pub use crate::rename_finders::MetadataRenameFinder;
pub use crate::rename_finders::RenameFinder;
pub use crate::utils::is_content_similar;

#[cfg(test)]
mod tests;

/// SearchDirection when searching renames.
///
/// Assuming we have a commit graph like below:
///
///  a..z # draw dag syntax
///
/// Forward means searching from a to z.
/// Backward means searching from z to a.
#[derive(Debug, PartialEq, Copy, Clone, Eq, Hash)]
pub(crate) enum SearchDirection {
    Forward,
    Backward,
}
