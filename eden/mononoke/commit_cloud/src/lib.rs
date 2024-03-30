/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use sql_ext::SqlConnections;

use crate::heads::WorkspaceHead;
use crate::history::WorkspaceHistory;
use crate::local_bookmarks::WorkspaceLocalBookmark;
use crate::remote_bookmarks::WorkspaceRemoteBookmark;

pub mod builder;
pub(crate) mod checkout_locations;
pub(crate) mod heads;
pub(crate) mod history;
pub(crate) mod local_bookmarks;
pub(crate) mod remote_bookmarks;
pub(crate) mod snapshots;
pub(crate) mod versions;

#[allow(unused)]
pub(crate) struct WorkspaceContents {
    heads: Vec<WorkspaceHead>,
    local_bookmarks: Vec<WorkspaceLocalBookmark>,
    remote_bookmarks: Vec<WorkspaceRemoteBookmark>,
    history: WorkspaceHistory,
}

pub struct SqlCommitCloud {
    #[allow(unused)]
    pub(crate) connections: SqlConnections,
}

impl SqlCommitCloud {
    pub fn new(connections: SqlConnections) -> Self {
        Self { connections }
    }
}
