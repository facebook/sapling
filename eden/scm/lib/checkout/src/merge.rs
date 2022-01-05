/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::fmt;

use anyhow::bail;
use anyhow::Result;
use manifest::FileType;
use manifest::FsNodeMetadata;
use manifest::Manifest;
use pathmatcher::AlwaysMatcher;
use types::RepoPathBuf;

use crate::actions::Action;
use crate::actions::ActionMap;
use crate::actions::UpdateAction;
use crate::conflict::Conflict;
use crate::conflict::ConflictState;

/// Merge operation settings
pub struct Merge {}

/// Contains result of the mere, separated by update actions and conflicts
pub struct MergeResult<M: Manifest> {
    dest: M,
    actions: ActionMap,
    conflicts: ConflictState,
}

pub enum ActionOrConflict {
    Action(Action),
    Conflict(Conflict),
}

impl Merge {
    // dest          result
    // |             |
    // |  src   =>   dest
    // |  /          |
    // base          base
    // in terms of merge.py:
    //    'dest' = local  = m1/n1/fl1
    //    'src'  = remote = m2/n2/fl2
    pub fn merge<M: Manifest + Clone>(
        &self,
        src: &M,
        dest: &M,
        base: &M,
    ) -> Result<MergeResult<M>> {
        let matcher = AlwaysMatcher::new();
        let diff = base.diff(dest, &matcher)?;
        let dest_actions = ActionMap::from_diff(diff)?;
        let diff = base.diff(src, &matcher)?;
        let src_actions = ActionMap::from_diff(diff)?;
        let dest_files: HashSet<_> = dest_actions.keys().collect();
        let src_files = src_actions.keys().collect();
        let union = dest_files.union(&src_files);
        let mut result = MergeResult::new_empty(dest.clone());
        for file in union {
            let ac = match (src_actions.get(*file), dest_actions.get(*file)) {
                (None, Some(_a)) => continue, // Already in destination
                (Some(a), None) => ActionOrConflict::Action(*a),
                (Some(s), Some(d)) => {
                    if let Some(ac) = self.resolve_conflict(*s, *d) {
                        ac
                    } else {
                        continue;
                    }
                }
                (None, None) => unreachable!(),
            };
            let file = (*file).clone();
            result.insert_new(file, ac);
        }
        Ok(result)
    }

    fn resolve_conflict(&self, src: Action, dest: Action) -> Option<ActionOrConflict> {
        Some(match (src, dest) {
            (Action::Remove, Action::Remove) => return None,
            (Action::Update(s), Action::Update(d)) => both_changed(s, d),
            (Action::UpdateExec(s), Action::UpdateExec(d)) => {
                assert!(s == d); // Can not be otherwise
                ActionOrConflict::Action(Action::UpdateExec(s))
            }

            // exists only in dest / local / m1
            (Action::Update(up), Action::Remove) => {
                ActionOrConflict::Conflict(Conflict::DstRemovedSrcChanged(up))
            }

            // mercurial handles this differently - it actually raise it as a conflict since file has "changed" and removed
            // but more logical is probably to just remove the file and ignore flag update on the other side
            (Action::UpdateExec(_), Action::Remove) => return None,

            // exists only in src / remote / m2
            (Action::Remove, Action::Update(up)) => {
                ActionOrConflict::Conflict(Conflict::SrcRemovedDstChanged(up))
            }
            // same as mirror case above, for mercurial this is a conflict
            (Action::Remove, Action::UpdateExec(_)) => ActionOrConflict::Action(Action::Remove),

            // flag conflicts - todo - implement
            (Action::Update(_), Action::UpdateExec(_)) => unimplemented!(),
            (Action::UpdateExec(_), Action::Update(_)) => unimplemented!(),
        })
    }
}

fn both_changed(src: UpdateAction, dest: UpdateAction) -> ActionOrConflict {
    assert_eq!(dest.from, src.from);
    ActionOrConflict::Conflict(Conflict::BothChanged {
        ancestor: dest.from,
        src: src.to,
        dest: dest.to,
    })
}

impl<M: Manifest> MergeResult<M> {
    pub fn new_empty(dest: M) -> Self {
        Self {
            dest,
            actions: Default::default(),
            conflicts: Default::default(),
        }
    }

    pub fn insert_new(&mut self, file: RepoPathBuf, ac: ActionOrConflict) {
        match ac {
            ActionOrConflict::Action(action) => {
                self.actions.insert(file, action);
            }
            ActionOrConflict::Conflict(conflict) => {
                self.conflicts.insert(file, conflict);
            }
        }
    }

    pub fn has_conflicts(&self) -> bool {
        !self.conflicts.is_empty()
    }

    /// If MergeResult has no conflicts, it can be directly converted into Manifest.
    pub fn try_into_manifest(self) -> Result<Option<M>> {
        if self.has_conflicts() {
            return Ok(None);
        }
        let mut m = self.dest;
        let (removes, updates): (Vec<_>, Vec<_>) = self
            .actions
            .into_iter()
            .partition(|(_, x)| matches!(x, Action::Remove));
        for (file, _) in removes {
            m.remove(&file)?;
        }
        for (file, action) in updates {
            match action {
                Action::Update(up) => {
                    m.insert(file, up.to)?;
                }
                Action::Remove => unreachable!(),
                Action::UpdateExec(up) => {
                    let meta = m.get(&file)?;
                    let mut meta = match meta {
                        Some(FsNodeMetadata::File(m)) => m,
                        Some(FsNodeMetadata::Directory(_)) => {
                            bail!(
                                "Failed to apply to manifest: {} is a directory, expected file",
                                file
                            )
                        }
                        None => bail!("Failed to apply to manifest: {} not found", file),
                    };

                    assert!(meta.file_type != FileType::Symlink);
                    if up {
                        meta.file_type = FileType::Executable;
                    } else {
                        meta.file_type = FileType::Regular;
                    }

                    m.insert(file, meta)?;
                }
            }
        }
        Ok(Some(m))
    }

    pub fn try_actions(&self) -> Option<&ActionMap> {
        if self.has_conflicts() {
            return None;
        }
        Some(&self.actions)
    }

    pub fn conflicts(&self) -> &ConflictState {
        &self.conflicts
    }

    pub fn actions(&self) -> &ActionMap {
        &self.actions
    }

    pub fn into_actions_and_conflicts(self) -> (ActionMap, ConflictState) {
        (self.actions, self.conflicts)
    }
}

impl<T: Manifest> fmt::Display for MergeResult<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}\n{}", self.actions, self.conflicts)
    }
}
