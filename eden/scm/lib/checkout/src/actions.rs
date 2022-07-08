/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fmt;
use std::ops::Deref;
use std::ops::DerefMut;
use std::sync::Arc;

use anyhow::Result;
use manifest::DiffEntry;
use manifest::DiffType;
use manifest::FileMetadata;
use manifest::FileType;
use manifest::Manifest;
use pathmatcher::Matcher;
use pathmatcher::XorMatcher;
use progress_model::ProgressBar;
use tracing::instrument;
use types::RepoPathBuf;

/// Map of simple actions that needs to be performed to move between revisions without conflicts.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ActionMap {
    map: HashMap<RepoPathBuf, Action>,
}

/// Basic update action.
/// Diff between regular(no conflict checkin) commit generates list of such actions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    Update(UpdateAction),
    Remove,
    UpdateExec(bool),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UpdateAction {
    pub from: Option<FileMetadata>,
    pub to: FileMetadata,
}

impl ActionMap {
    // This is similar to CheckoutPlan::new
    // Eventually CheckoutPlan::new will migrate to take (Conflict)ActionMap instead of a Diff and there won't be code duplication
    #[instrument(skip_all)]
    pub fn from_diff<D: Iterator<Item = Result<DiffEntry>>>(diff: D) -> Result<Self> {
        let mut map = HashMap::new();
        for entry in diff {
            let entry = entry?;
            match entry.diff_type {
                DiffType::LeftOnly(_) => {
                    map.insert(entry.path, Action::Remove);
                }
                DiffType::RightOnly(meta) => {
                    if meta.file_type != FileType::GitSubmodule {
                        map.insert(entry.path, Action::Update(UpdateAction::new(None, meta)));
                    }
                }
                DiffType::Changed(old, new) => {
                    match (old.hgid == new.hgid, old.file_type, new.file_type) {
                        (true, FileType::Executable, FileType::Regular) => {
                            map.insert(entry.path, Action::UpdateExec(false));
                        }
                        (true, FileType::Regular, FileType::Executable) => {
                            map.insert(entry.path, Action::UpdateExec(true));
                        }
                        _ => {
                            if new.file_type != FileType::GitSubmodule {
                                map.insert(
                                    entry.path,
                                    Action::Update(UpdateAction::new(Some(old), new)),
                                );
                            }
                        }
                    }
                }
            }
        }
        Ok(Self { map })
    }

    pub fn with_sparse_profile_change<
        M1: 'static + Matcher + Send + Sync,
        M2: 'static + Matcher + Send + Sync,
    >(
        mut self,
        old_matcher: M1,
        new_matcher: M2,
        old_manifest: &impl Manifest,
        new_manifest: &impl Manifest,
    ) -> Result<Self> {
        let _prog = ProgressBar::register_new("sparse config", 0, "");

        // First - remove all the files that were scheduled for update, but actually aren't in new sparse profile
        let mut result = Ok(());
        self.map.retain(|path, action| {
            if result.is_err() {
                return true;
            }
            if matches!(action, Action::Remove) {
                return true;
            }
            match new_matcher.matches_file(path.as_ref()) {
                Ok(v) => v,
                Err(err) => {
                    result = Err(err);
                    true
                }
            }
        });
        result?;

        // Second - handle files in a new manifest, that were affected by sparse profile change
        let new_matcher = Arc::new(new_matcher);
        let xor_matcher = XorMatcher::new(old_matcher, new_matcher.clone());
        for file in new_manifest.files(xor_matcher) {
            let file = file?;
            if new_matcher.matches_file(&file.path)? {
                match self.map.entry(file.path) {
                    Entry::Vacant(va) => {
                        va.insert(Action::Update(UpdateAction::new(None, file.meta)));
                    }
                    Entry::Occupied(mut oc) => match oc.get() {
                        Action::Remove | Action::Update(_) => {}
                        Action::UpdateExec(_) => {
                            oc.insert(Action::Update(UpdateAction::new(None, file.meta)));
                        }
                    },
                }
            } else {
                // By definition of xor matcher this means old_matcher.matches_file==true.
                // Remove it if it existed before.
                if old_manifest.get(&file.path)?.is_some() {
                    self.map.insert(file.path, Action::Remove);
                }
            }
        }

        Ok(self)
    }

    #[cfg(test)]
    pub fn empty() -> Self {
        Self {
            map: Default::default(),
        }
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
        // NOTE: hg does not have git submoduel "type". Is this code path actually used?
        FileType::GitSubmodule => "",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use manifest_tree::testutil::make_tree_manifest_from_meta;
    use manifest_tree::testutil::TestStore;
    use pathmatcher::TreeMatcher;
    use types::HgId;

    use super::*;

    #[test]
    fn test_with_sparse_profile_change() -> Result<()> {
        let store = Arc::new(TestStore::new());
        let a = (rp("a"), FileMetadata::regular(hgid(1)));
        let b = (rp("b"), FileMetadata::regular(hgid(2)));
        let c = (rp("c"), FileMetadata::regular(hgid(3)));
        let ab_profile = Arc::new(TreeMatcher::from_rules(["a", "b"].iter())?);
        let ac_profile = Arc::new(TreeMatcher::from_rules(["a", "c"].iter())?);
        let old_manifest = make_tree_manifest_from_meta(store.clone(), vec![]);
        let manifest = make_tree_manifest_from_meta(store, vec![a, b, c]);

        let actions = ActionMap::empty().with_sparse_profile_change(
            ab_profile.clone(),
            ab_profile.clone(),
            &old_manifest,
            &manifest,
        )?;
        assert_eq!("", &actions.to_string());

        let mut expected_actions = ActionMap::empty();
        expected_actions.map.insert(
            rp("c"),
            Action::Update(UpdateAction::new(None, FileMetadata::regular(hgid(3)))),
        );

        let actions = ActionMap::empty().with_sparse_profile_change(
            ab_profile.clone(),
            ac_profile.clone(),
            &old_manifest,
            &manifest,
        )?;
        assert_eq!(expected_actions, actions);

        let mut actions = ActionMap::empty();
        actions.map.insert(
            rp("b"),
            Action::Update(UpdateAction::new(None, FileMetadata::regular(hgid(10)))),
        );
        actions.map.insert(rp("b"), Action::UpdateExec(true));
        let actions = actions.with_sparse_profile_change(
            ab_profile.clone(),
            ac_profile.clone(),
            &old_manifest,
            &manifest,
        )?;
        assert_eq!(expected_actions, actions);

        let mut actions = ActionMap::empty();
        actions.map.insert(
            rp("c"),
            Action::Update(UpdateAction::new(None, FileMetadata::regular(hgid(3)))),
        );
        let actions = actions.with_sparse_profile_change(
            ab_profile.clone(),
            ac_profile.clone(),
            &old_manifest,
            &manifest,
        )?;

        assert_eq!(expected_actions, actions);

        Ok(())
    }

    fn rp(p: &str) -> RepoPathBuf {
        RepoPathBuf::from_string(p.to_string()).unwrap()
    }

    fn hgid(p: u8) -> HgId {
        let mut r = HgId::default().into_byte_array();
        r[0] = p;
        HgId::from_byte_array(r)
    }
}
