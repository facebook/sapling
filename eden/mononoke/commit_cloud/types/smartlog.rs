/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mercurial_types::HgChangesetId;

use super::WorkspaceRemoteBookmark;
pub struct SmartlogNode {
    pub node: HgChangesetId,
    pub phase: String,
    pub author: String,
    pub date: i64,
    pub message: String,
    pub parents: Vec<HgChangesetId>,
    pub bookmarks: Vec<String>,
    pub remote_bookmarks: Option<Vec<WorkspaceRemoteBookmark>>,
}

pub struct SmartlogData {
    pub nodes: Vec<SmartlogNode>,
    pub version: Option<i64>,
    pub timestamp: Option<i64>,
}

pub enum SmartlogFilter {
    Version(i64),
    Timestamp(i64),
}

#[derive(PartialEq)]
pub enum SmartlogFlag {
    SkipPublicCommitsMetadata,
    AddRemoteBookmarks,
    AddAllBookmarks,
}
