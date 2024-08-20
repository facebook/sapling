/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::Timestamp;

use super::RawReferencesData;
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

impl WorkspaceHistory {
    pub fn from_references(
        raw: RawReferencesData,
        version: u64,
        timestamp: i64,
    ) -> WorkspaceHistory {
        Self {
            version,
            timestamp: if timestamp == 0 {
                None
            } else {
                Some(Timestamp::from_timestamp_nanos(timestamp))
            },
            heads: raw.heads,
            local_bookmarks: raw.local_bookmarks,
            remote_bookmarks: raw.remote_bookmarks,
        }
    }
}
