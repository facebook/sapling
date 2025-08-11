/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Log changes to the repository (new commits and bookmark updates) to
//! external telemetry.

mod bookmark_logger;
mod commit_logger;

pub use crate::bookmark_logger::BookmarkInfo;
pub use crate::bookmark_logger::BookmarkOperation;
pub use crate::bookmark_logger::GitContentRefInfo;
pub use crate::bookmark_logger::PlainBookmarkInfo;
pub use crate::bookmark_logger::log_bookmark_operation;
pub use crate::bookmark_logger::log_git_content_ref;
pub use crate::commit_logger::CommitInfo;
pub use crate::commit_logger::extract_differential_revision;
pub use crate::commit_logger::find_draft_ancestors;
pub use crate::commit_logger::log_new_commits;
