/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::sql::heads::WorkspaceHead;
use crate::sql::history::WorkspaceHistory;
use crate::sql::local_bookmarks::WorkspaceLocalBookmark;
use crate::sql::remote_bookmarks::WorkspaceRemoteBookmark;

#[allow(unused)]
pub(crate) struct WorkspaceContents {
    heads: Vec<WorkspaceHead>,
    local_bookmarks: Vec<WorkspaceLocalBookmark>,
    remote_bookmarks: Vec<WorkspaceRemoteBookmark>,
    history: WorkspaceHistory,
}
