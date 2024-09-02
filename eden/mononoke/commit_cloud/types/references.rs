/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

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
