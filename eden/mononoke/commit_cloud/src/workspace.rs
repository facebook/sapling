/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::sql::heads_ops::WorkspaceHead;
use crate::sql::history_ops::WorkspaceHistory;
use crate::sql::local_bookmarks_ops::WorkspaceLocalBookmark;
use crate::sql::remote_bookmarks_ops::WorkspaceRemoteBookmark;

#[allow(unused)]
pub(crate) struct WorkspaceContents {
    heads: Vec<WorkspaceHead>,
    local_bookmarks: Vec<WorkspaceLocalBookmark>,
    remote_bookmarks: Vec<WorkspaceRemoteBookmark>,
    history: WorkspaceHistory,
}
