/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::Timestamp;

use crate::references::heads::WorkspaceHead;
use crate::references::local_bookmarks::WorkspaceLocalBookmark;
use crate::references::remote_bookmarks::WorkspaceRemoteBookmark;

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceHistory {
    pub version: u64,
    pub timestamp: Option<Timestamp>,
    pub heads: Vec<WorkspaceHead>,
    pub local_bookmarks: Vec<WorkspaceLocalBookmark>,
    pub remote_bookmarks: Vec<WorkspaceRemoteBookmark>,
}
