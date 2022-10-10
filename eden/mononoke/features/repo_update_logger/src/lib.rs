/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Log changes to the repository (new commits and bookmark updates) to
//! external telemetry.

mod bookmark_logger;

pub use bookmark_logger::log_bookmark_operation;
pub use bookmark_logger::BookmarkInfo;
pub use bookmark_logger::BookmarkOperation;
