/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    convert::{TryFrom, TryInto},
    path::PathBuf,
    time::SystemTime,
};

use anyhow::{Error, Result};
use pathmatcher::Matcher;
use types::RepoPathBuf;
use vfs::VFS;

/// Represents a file modification time in Mercurial, in seconds since the unix epoch.
#[derive(PartialEq)]
pub struct HgModifiedTime(u64);

impl From<u64> for HgModifiedTime {
    fn from(value: u64) -> Self {
        HgModifiedTime(value)
    }
}

impl From<u32> for HgModifiedTime {
    fn from(value: u32) -> Self {
        HgModifiedTime(value.into())
    }
}

impl TryFrom<SystemTime> for HgModifiedTime {
    type Error = Error;
    fn try_from(value: SystemTime) -> Result<Self> {
        Ok(value
            .duration_since(SystemTime::UNIX_EPOCH)?
            .as_secs()
            .into())
    }
}

impl TryFrom<i32> for HgModifiedTime {
    type Error = Error;
    fn try_from(value: i32) -> Result<Self> {
        Ok(HgModifiedTime(value.try_into()?))
    }
}

pub struct PhysicalFileSystem {
    // TODO: Make this an Arc<Mutex<VFS>> so we can persist the vfs pathauditor cache
    vfs: VFS,
}

impl PhysicalFileSystem {
    pub fn new(root: PathBuf) -> Result<Self> {
        Ok(PhysicalFileSystem {
            vfs: VFS::new(root)?,
        })
    }

    pub fn pending_changes<M: Matcher + Clone>(&self, matcher: M) -> PendingChanges<M> {
        PendingChanges {
            vfs: self.vfs.clone(),
            matcher,
            stage: PendingChangesStage::Walk,
        }
    }
}

pub struct PendingChanges<M: Matcher + Clone> {
    vfs: VFS,
    matcher: M,
    stage: PendingChangesStage,
}

#[derive(PartialEq)]
enum PendingChangesStage {
    Walk,
    IterateTree,
    Lookups,
    Finished,
}

impl PendingChangesStage {
    pub fn next(&self) -> PendingChangesStage {
        match self {
            PendingChangesStage::Walk => PendingChangesStage::IterateTree,
            PendingChangesStage::IterateTree => PendingChangesStage::Lookups,
            PendingChangesStage::Lookups => PendingChangesStage::Finished,
            PendingChangesStage::Finished => PendingChangesStage::Finished,
        }
    }
}

pub enum ChangeType {
    Changed(RepoPathBuf),
    Deleted(RepoPathBuf),
}

pub enum PendingChangeResult {
    File(ChangeType),
    SeenDirectory(RepoPathBuf),
}

impl<M: Matcher + Clone> PendingChanges<M> {
    fn next_walk(&mut self) -> Option<Result<PendingChangeResult>> {
        None
    }

    fn next_tree(&mut self) -> Option<Result<PendingChangeResult>> {
        None
    }

    fn next_lookup(&mut self) -> Option<Result<PendingChangeResult>> {
        None
    }
}

impl<M: Matcher + Clone> Iterator for PendingChanges<M> {
    type Item = Result<PendingChangeResult>;

    fn next(&mut self) -> Option<Self::Item> {
        // TODO: Try to make this into a chain instead of a manual state machine
        loop {
            let change = match self.stage {
                PendingChangesStage::Walk => self.next_walk(),
                PendingChangesStage::IterateTree => self.next_tree(),
                PendingChangesStage::Lookups => self.next_lookup(),
                PendingChangesStage::Finished => None,
            };

            if change.is_some() {
                return change;
            }

            self.stage = self.stage.next();
            if self.stage == PendingChangesStage::Finished {
                return None;
            }
        }
    }
}
