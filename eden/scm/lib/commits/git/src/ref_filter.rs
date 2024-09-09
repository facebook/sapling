/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::LazyLock;

use anyhow::Result;
use gitcompat::ReferenceValue;
use types::HgId;

/// Help decide whether a Git ref should be imported or not.
pub(crate) struct GitRefFilter<'a> {
    refs: &'a BTreeMap<String, ReferenceValue>,
    // e.g. "origin/main"
    head_remotenames: HashSet<&'a str>,
    // e.g. "origin/main"
    existing_remotenames: &'a BTreeMap<String, HgId>,
    // e.g. "main", without "origin".
    selective_pull_default: &'a HashSet<String>,
    // e.g. "origin"
    hoist: &'a str,
    is_dotgit: bool,
}

impl<'a> GitRefFilter<'a> {
    /// Initialize for dotgit use-case.
    pub(crate) fn new_for_dotgit(
        refs: &'a BTreeMap<String, ReferenceValue>,
        existing_remotenames: &'a BTreeMap<String, HgId>,
        hoist: Option<&'a str>,
        selective_pull_default: &'a HashSet<String>,
    ) -> Result<Self> {
        Self::new(
            refs,
            existing_remotenames,
            hoist.unwrap_or("origin"),
            selective_pull_default,
            true,
        )
    }

    /// Initialize for non-dotgit use-case.
    pub(crate) fn new_for_dotsl(refs: &'a BTreeMap<String, ReferenceValue>) -> Result<Self> {
        // No need to use "existing_remotenames" for non-dotgit.
        static EMPTY_MAP: BTreeMap<String, HgId> = BTreeMap::new();
        static EMPTY_SET: LazyLock<HashSet<String>> = LazyLock::new(Default::default);
        Self::new(refs, &EMPTY_MAP, "remote", &EMPTY_SET, false)
    }

    fn new(
        refs: &'a BTreeMap<String, ReferenceValue>,
        existing_remotenames: &'a BTreeMap<String, HgId>,
        hoist: &'a str,
        selective_pull_default: &'a HashSet<String>,
        is_dotgit: bool,
    ) -> Result<Self> {
        // e.g. "origin/main"
        let mut head_remotenames = HashSet::new();
        // Select "default" remote branches. Some remote repos might have
        // uncommon default branch names like "v1", they don't match the
        // selective pull config, or the existing remotenames. But they
        // should be synced.
        // e.g. "refs/remotes/origin/HEAD" to "ref: refs/remotes/origin/master".
        let prefix = "refs/remotes/";
        for (ref_name, value) in refs.range(prefix.to_string()..) {
            if !ref_name.starts_with(prefix) {
                break;
            }
            if ref_name.ends_with("/HEAD") {
                if let ReferenceValue::Sym(target) = value {
                    if let Some(rest) = target.strip_prefix("refs/remotes/") {
                        tracing::trace!("select remote HEAD ref: {}", rest);
                        head_remotenames.insert(rest);
                    }
                }
            }
        }

        Ok(Self {
            refs,
            head_remotenames,
            existing_remotenames,
            selective_pull_default,
            hoist,
            is_dotgit,
        })
    }

    /// Decides whether "refs/remotes/<name>" should be imported or not.
    pub(crate) fn should_import_remote_name(&self, name: &str) -> bool {
        if !name.contains('/') || name.ends_with("/HEAD") || name.starts_with("tags/") {
            return false;
        }
        if self.is_dotgit {
            if self.existing_remotenames.contains_key(name) || self.head_remotenames.contains(name)
            {
                return true;
            }
            if let Some((_remote, rest)) = name.split_once('/') {
                if self.selective_pull_default.contains(rest) {
                    return true;
                }
            }
            false
        } else {
            true
        }
    }

    /// Test if "refs/reomtes/<name>" is a "main" remote name (pointed by HEAD).
    pub(crate) fn is_main_remote_name(&self, name: &str) -> bool {
        self.head_remotenames.contains(name)
    }

    /// Decides whether "refs/heads/<name>" should be imported as a (unnamed) visiblehead.
    /// This is a workflow difference between Sapling and Git. Sapling disallows and does not
    /// encourage local bookmarks like "main", or match the remotename. The name "main" in
    /// Sapling usually is just an alias to "remote/main". A local "main" causes confusion.
    pub(crate) fn should_treat_local_ref_as_visible_head(&self, name: &str) -> bool {
        if self.is_dotgit {
            // Common disallow names. They can be useful when the repo does not have a remote.
            // NOTE: Consider moving this to a config and also consider Git's init.defaultBranch.
            const DISALLOW_BOOKMARK_NAMES: &[&str] = &["main", "master", "HEAD"];
            if DISALLOW_BOOKMARK_NAMES.contains(&name) {
                tracing::trace!(name, "treat local head as unnamed (builtin)");
                return true;
            }
            let ref_name = format!("refs/remotes/{}/{}", self.hoist, name);
            if self.refs.contains_key(&ref_name) {
                tracing::trace!(name, ref_name, "treat local head as unnamed (git ref)");
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ref_filter_dotgit() {
        let refs = get_test_refs("origin");

        let mut existing_remotenames = BTreeMap::new();
        existing_remotenames.insert("origin/b2".to_string(), *HgId::null_id());

        let mut selected = HashSet::new();
        selected.insert("b3".to_string());

        let filter =
            GitRefFilter::new_for_dotgit(&refs, &existing_remotenames, None, &selected).unwrap();
        // referred by HEAD (default)
        assert!(filter.should_import_remote_name("origin/b1"));
        // matches existing
        assert!(filter.should_import_remote_name("origin/b2"));
        // matches config
        assert!(filter.should_import_remote_name("origin/b3"));
        // matches nothing
        assert!(!filter.should_import_remote_name("origin/b4"));
        assert!(filter.should_treat_local_ref_as_visible_head("main"));
        assert!(filter.should_treat_local_ref_as_visible_head("b1"));
        assert!(filter.should_treat_local_ref_as_visible_head("b4"));
        // detects main branch
        assert!(filter.is_main_remote_name("origin/b1"));
        assert!(!filter.is_main_remote_name("origin/b2"));
    }

    #[test]
    fn test_ref_filter_non_dotgit() {
        let refs = get_test_refs("remote");
        let filter = GitRefFilter::new_for_dotsl(&refs).unwrap();
        // all local refs should be treated as bookmarks since Git does not write
        assert!(!filter.should_treat_local_ref_as_visible_head("main"));
        assert!(!filter.should_treat_local_ref_as_visible_head("foo"));
        // all remote names should be imported. Except */HEAD
        assert!(filter.should_import_remote_name("remote/main"));
        assert!(filter.should_import_remote_name("remote/foo"));
        assert!(filter.should_import_remote_name("upstream/bar"));
        assert!(!filter.should_import_remote_name("upstream/HEAD"));
        //
        assert!(filter.is_main_remote_name("remote/b1"));
        assert!(!filter.is_main_remote_name("remote/b2"));
    }

    fn get_test_refs(remote: &str) -> BTreeMap<String, ReferenceValue> {
        let mut refs = BTreeMap::new();
        let id = *HgId::null_id();
        let r = |name: &str| format!("refs/remotes/{remote}/{name}");
        refs.insert(
            r("HEAD"),
            ReferenceValue::Sym(format!("refs/remotes/{remote}/b1")),
        );
        refs.insert(r("b1"), ReferenceValue::Id(id));
        refs.insert(r("b2"), ReferenceValue::Id(id));
        refs.insert(r("b3"), ReferenceValue::Id(id));
        refs.insert(r("b4"), ReferenceValue::Id(id));
        refs
    }
}
