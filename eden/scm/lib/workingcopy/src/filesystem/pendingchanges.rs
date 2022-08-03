/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use pathmatcher::Matcher;
use serde::Serialize;
use types::RepoPathBuf;

#[derive(Serialize)]
pub enum ChangeType {
    Changed(RepoPathBuf),
    Deleted(RepoPathBuf),
}

impl ChangeType {
    pub fn get_path(&self) -> &RepoPathBuf {
        match self {
            ChangeType::Changed(path) => path,
            ChangeType::Deleted(path) => path,
        }
    }
}

#[derive(Serialize)]
pub enum PendingChangeResult {
    File(ChangeType),
    SeenDirectory(RepoPathBuf),
}

pub trait PendingChanges {
    fn pending_changes<M>(
        &self,
        matcher: M,
    ) -> Result<Box<dyn Iterator<Item = Result<PendingChangeResult>>>>
    where
        M: Matcher + Clone + Send + Sync + 'static;
}
