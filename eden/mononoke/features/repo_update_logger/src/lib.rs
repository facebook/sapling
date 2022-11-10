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

pub use crate::bookmark_logger::log_bookmark_operation;
pub use crate::bookmark_logger::BookmarkInfo;
pub use crate::bookmark_logger::BookmarkOperation;
pub use crate::commit_logger::log_new_commits;
pub use crate::commit_logger::CommitInfo;
