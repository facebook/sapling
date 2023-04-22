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

// traits
pub use crate::copy_trace::CopyTrace;
// copy tracers
pub use crate::dag_copy_trace::DagCopyTrace;
pub use crate::git_copy_trace::GitCopyTrace;
// rename finders
pub use crate::rename_finders::RenameFinder;
pub use crate::rename_finders::SaplingRenameFinder;

#[cfg(test)]
mod tests;
