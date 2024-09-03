/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::ensure;
use mercurial_types::HgChangesetId;
use mononoke_types::Timestamp;
use serde::Deserialize;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceHead {
    pub commit: HgChangesetId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceCheckoutLocation {
    pub hostname: String,
    pub commit: HgChangesetId,
    pub checkout_path: PathBuf,
    pub shared_path: PathBuf,
    pub timestamp: Timestamp,
    pub unixname: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceSnapshot {
    pub commit: HgChangesetId,
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct WorkspaceLocalBookmark {
    name: String,
    commit: HgChangesetId,
}

impl WorkspaceLocalBookmark {
    pub fn new(name: String, commit: HgChangesetId) -> anyhow::Result<Self> {
        ensure!(
            !name.is_empty(),
            "'commit cloud' failed: Local bookmark name cannot be empty"
        );

        Ok(Self { name, commit })
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn commit(&self) -> &HgChangesetId {
        &self.commit
    }
}

pub type LocalBookmarksMap = HashMap<HgChangesetId, Vec<String>>;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceRemoteBookmark {
    name: String,
    commit: HgChangesetId,
    remote: String,
}

impl WorkspaceRemoteBookmark {
    pub fn new(remote: String, name: String, commit: HgChangesetId) -> anyhow::Result<Self> {
        ensure!(
            !name.is_empty(),
            "'commit cloud' failed: remote bookmark name cannot be empty"
        );
        ensure!(
            !remote.is_empty(),
            "'commit cloud' failed: remote bookmark 'remote' part cannot be empty"
        );
        Ok(Self {
            name,
            commit,
            remote,
        })
    }

    pub fn name(&self) -> &String {
        &self.name
    }

    pub fn commit(&self) -> &HgChangesetId {
        &self.commit
    }

    pub fn remote(&self) -> &String {
        &self.remote
    }

    pub fn full_name(&self) -> String {
        format!("{}/{}", self.remote, self.name)
    }
}

pub type RemoteBookmarksMap = HashMap<HgChangesetId, Vec<WorkspaceRemoteBookmark>>;

pub struct ReferencesData {
    pub version: u64,
    pub heads: Option<Vec<HgChangesetId>>,
    pub bookmarks: Option<HashMap<String, HgChangesetId>>,
    pub heads_dates: Option<HashMap<HgChangesetId, i64>>,
    pub remote_bookmarks: Option<Vec<WorkspaceRemoteBookmark>>,
    pub snapshots: Option<Vec<HgChangesetId>>,
    pub timestamp: Option<i64>,
}

#[derive(Clone)]
pub struct UpdateReferencesParams {
    pub workspace: String,
    pub reponame: String,
    pub version: u64,
    pub removed_heads: Vec<HgChangesetId>,
    pub new_heads: Vec<HgChangesetId>,
    pub updated_bookmarks: HashMap<String, HgChangesetId>,
    pub removed_bookmarks: Vec<String>,
    pub updated_remote_bookmarks: Option<Vec<WorkspaceRemoteBookmark>>,
    pub removed_remote_bookmarks: Option<Vec<WorkspaceRemoteBookmark>>,
    pub new_snapshots: Vec<HgChangesetId>,
    pub removed_snapshots: Vec<HgChangesetId>,
    pub client_info: Option<ClientInfo>,
}

#[derive(Debug, Clone)]
pub struct ClientInfo {
    pub hostname: String,
    pub reporoot: String,
    pub version: u64,
}
