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
