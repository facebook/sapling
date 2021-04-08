/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use manifest::{DiffType, FileMetadata, FileType, Manifest};
use pathmatcher::AlwaysMatcher;
use std::collections::HashMap;
use std::fmt;
use std::ops::{Deref, DerefMut};
use types::RepoPathBuf;

/// Map of simple actions that needs to be performed to move between revisions without conflicts.
#[derive(Default)]
pub struct ActionMap {
    map: HashMap<RepoPathBuf, Action>,
}

/// Basic update action.
/// Diff between regular(no conflict checkin) commit generates list of such actions.
#[derive(Clone, Copy)]
pub enum Action {
    Update(UpdateAction),
    Remove,
    UpdateExec(bool),
}

#[derive(Clone, Copy)]
pub struct UpdateAction {
    pub from: Option<FileMetadata>,
    pub to: FileMetadata,
}

impl ActionMap {
    // This is similar to CheckoutPlan::new
    // Eventually CheckoutPlan::new will migrate to take (Conflict)ActionMap instead of a Diff and there won't be code duplication
    pub fn from_diff<M: Manifest>(src: &M, dst: &M) -> Result<Self> {
        let matcher = AlwaysMatcher::new();
        let diff = src.diff(dst, &matcher);
        let mut map = HashMap::new();
        for entry in diff {
            let entry = entry?;
            match entry.diff_type {
                DiffType::LeftOnly(_) => map.insert(entry.path, Action::Remove),
                DiffType::RightOnly(meta) => {
                    map.insert(entry.path, Action::Update(UpdateAction::new(None, meta)))
                }
                DiffType::Changed(old, new) => {
                    match (old.hgid == new.hgid, old.file_type, new.file_type) {
                        (true, FileType::Executable, FileType::Regular) => {
                            map.insert(entry.path, Action::UpdateExec(false))
                        }
                        (true, FileType::Regular, FileType::Executable) => {
                            map.insert(entry.path, Action::UpdateExec(true))
                        }
                        _ => map.insert(
                            entry.path,
                            Action::Update(UpdateAction::new(Some(old), new)),
                        ),
                    }
                }
            };
        }
        Ok(Self { map })
    }
}

impl Deref for ActionMap {
    type Target = HashMap<RepoPathBuf, Action>;

    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl DerefMut for ActionMap {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.map
    }
}

impl UpdateAction {
    pub fn new(from: Option<FileMetadata>, to: FileMetadata) -> Self {
        Self { from, to }
    }
}

impl IntoIterator for ActionMap {
    type Item = (RepoPathBuf, Action);
    type IntoIter = <HashMap<RepoPathBuf, Action> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl fmt::Display for ActionMap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (path, action) in &self.map {
            match action {
                Action::Update(up) => write!(f, "up {}=>{}\n", path, up.to.hgid)?,
                Action::UpdateExec(x) => write!(f, "{} {}\n", if *x { "+x" } else { "-x" }, path)?,
                Action::Remove => write!(f, "rm {}\n", path)?,
            }
        }
        Ok(())
    }
}

impl Action {
    pub fn pymerge_action(&self) -> (&'static str, (&'static str, bool), &'static str) {
        match self {
            Action::Update(up) => ("g", (pyflags(&up.to.file_type), false), "created/changed"),
            Action::Remove => ("r", ("", false), "deleted"),
            Action::UpdateExec(x) => (
                "e",
                (if *x { "x" } else { "" }, false),
                "update permissions",
            ),
        }
    }
}

fn pyflags(t: &FileType) -> &'static str {
    match t {
        FileType::Symlink => "l",
        FileType::Regular => "",
        FileType::Executable => "x",
    }
}
