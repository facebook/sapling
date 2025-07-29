/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::LazyLock;

use anyhow::Result;
use gitcompat::ReferenceValue;
use pathmatcher_types::Matcher;
use refencode::RefName;
use types::HgId;
use types::RepoPath;

/// Decide whether a Git ref should be imported to metalog or not.
///
/// Happens after `GitRefPreliminaryFilter`.
///
/// UX considerations:
/// - Local `master` tracking `origin/master`. While "standard" in Git,
///   in production users are often confused with 2 "master"s.
///
///
/// Scalability considerations:
/// - `smartlog` output size: worse than O(N). shouldn't output
///   thousands of lines (must limit output size).
/// - `phase` calculation: worse than O(N). shouldn't have thousands
///   of draft/public heads (might be optimizable).
/// - importing git commits to segmented changelog: worse than O(N).
///   shouldn't have thousands of heads to import (might be optimizable).
///
/// Decisions (might be unfamiliar to experienced Git users, but matches
/// Sapling's behavior in a monorepo):
/// - Turn the local `master` into an anonymous head if `origin/master`
///   also exists.
/// - Apply the "selective pull" idea, start with a limited set of remote
///   refs (like, just "master"). Extend the list later with extra `pull`s.
pub(crate) struct GitRefMetaLogFilter<'a> {
    refs: &'a BTreeMap<String, ReferenceValue>,
    // e.g. "origin/main"
    head_remotenames: HashSet<&'a str>,
    // e.g. "origin/main"
    existing_remotenames: &'a BTreeMap<RefName, HgId>,
    // e.g. "main", without "origin".
    selective_pull_default: &'a HashSet<String>,
    // e.g. "refs/remotes/origin/*", "refs/remotes/m/*"
    import_remote_refs: Option<&'a (dyn Matcher + Send + Sync)>,
    // e.g. "origin"
    hoist: &'a str,
    // commit hash referred by HEAD
    head_id: HgId,
    is_dotgit: bool,
}

impl<'a> GitRefMetaLogFilter<'a> {
    /// Initialize for dotgit use-case.
    pub(crate) fn new_for_dotgit(
        refs: &'a BTreeMap<String, ReferenceValue>,
        existing_remotenames: &'a BTreeMap<RefName, HgId>,
        hoist: Option<&'a str>,
        selective_pull_default: &'a HashSet<String>,
        import_remote_refs: Option<&'a (dyn Matcher + Send + Sync)>,
        head_id: HgId,
    ) -> Result<Self> {
        Self::new(
            refs,
            existing_remotenames,
            hoist.unwrap_or("origin"),
            selective_pull_default,
            import_remote_refs,
            head_id,
            true,
        )
    }

    /// Initialize for non-dotgit use-case.
    pub(crate) fn new_for_dotsl(refs: &'a BTreeMap<String, ReferenceValue>) -> Result<Self> {
        // No need to use "existing_remotenames" for non-dotgit.
        static EMPTY_MAP: BTreeMap<RefName, HgId> = BTreeMap::new();
        static EMPTY_SET: LazyLock<HashSet<String>> = LazyLock::new(Default::default);
        Self::new(
            refs,
            &EMPTY_MAP,
            "remote",
            &EMPTY_SET,
            None,
            *HgId::null_id(),
            false,
        )
    }

    fn new(
        refs: &'a BTreeMap<String, ReferenceValue>,
        existing_remotenames: &'a BTreeMap<RefName, HgId>,
        hoist: &'a str,
        selective_pull_default: &'a HashSet<String>,
        import_remote_refs: Option<&'a (dyn Matcher + Send + Sync)>,
        head_id: HgId,
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
            import_remote_refs,
            hoist,
            head_id,
            is_dotgit,
        })
    }

    /// Decides whether "refs/remotes/<name>" should be imported or not.
    pub(crate) fn should_import_remote_name(&self, name: &str, id: &HgId) -> Result<bool> {
        if !name.contains('/') || name.ends_with("/HEAD") || name.starts_with("tags/") {
            return Ok(false);
        }
        if self.is_dotgit {
            // `git clone` by default fetches all remote refs, which hurts scalability
            // (see struct-level docstring).
            // `git clone --single-branch` is closer to what we want, but it is not
            // the default, and we cannot rely on users knowing and using the flag.
            // `sl clone` ("dotsl", not "dotgit") only fetches limited remote refs.
            // It does not have this problem.
            if &self.head_id == id {
                tracing::trace!(name, "should be imported (hash matches HEAD)");
                return Ok(true);
            }
            if self.existing_remotenames.contains_key(name) {
                tracing::trace!(name, "should be imported (existing)");
                return Ok(true);
            }
            if self.head_remotenames.contains(name) {
                tracing::trace!(
                    name,
                    "should be imported (pointed by remotes/<remote>/HEAD)"
                );
                return Ok(true);
            }
            if let Some(matcher) = self.import_remote_refs {
                if let Ok(path) = RepoPath::from_str(name) {
                    if matcher.matches_file(path)? {
                        tracing::trace!(name, "should be imported (matches import_remote_refs)");
                        return Ok(true);
                    }
                }
            }
            if let Some((_remote, rest)) = name.split_once('/') {
                if self.selective_pull_default.contains(rest) {
                    tracing::trace!(name, "should be imported (matches selective_pull_default)");
                    return Ok(true);
                }
            }
            tracing::trace!(name, "should not be imported (dotgit)");
            Ok(false)
        } else {
            tracing::trace!(name, "should be imported (dotsl)");
            Ok(true)
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
    use pathmatcher::TreeMatcher;

    use super::*;

    #[test]
    fn test_ref_filter_dotgit() -> Result<()> {
        let refs = get_test_refs("origin");

        let mut existing_remotenames = BTreeMap::new();
        existing_remotenames.insert(RefName::try_from("origin/b2").unwrap(), *HgId::null_id());

        let mut selected = HashSet::new();
        selected.insert("b3".to_string());

        let import_remote_refs = TreeMatcher::from_rules(std::iter::once("m/*"), false)?;
        let head_id = HgId::from_hex(b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
        let filter = GitRefMetaLogFilter::new_for_dotgit(
            &refs,
            &existing_remotenames,
            None,
            &selected,
            Some(&import_remote_refs),
            head_id,
        )
        .unwrap();
        let null_id = HgId::null_id();
        // referred by HEAD (default)
        assert!(filter.should_import_remote_name("origin/b1", null_id)?,);
        // matches existing
        assert!(filter.should_import_remote_name("origin/b2", null_id)?);
        // matches config
        assert!(filter.should_import_remote_name("origin/b3", null_id)?);
        // matches nothing
        assert!(!filter.should_import_remote_name("origin/b4", null_id)?);
        // name matches nothing, but id matches HEAD
        assert!(filter.should_import_remote_name("origin/b4", &head_id)?);
        // matches the import_remote_refs matcher
        assert!(filter.should_import_remote_name("m/foo", null_id)?);

        assert!(filter.should_treat_local_ref_as_visible_head("main"));
        assert!(filter.should_treat_local_ref_as_visible_head("b1"));
        assert!(filter.should_treat_local_ref_as_visible_head("b4"));
        // detects main branch
        assert!(filter.is_main_remote_name("origin/b1"));
        assert!(!filter.is_main_remote_name("origin/b2"));
        Ok(())
    }

    #[test]
    fn test_ref_filter_non_dotgit() -> Result<()> {
        let refs = get_test_refs("remote");
        let filter = GitRefMetaLogFilter::new_for_dotsl(&refs).unwrap();
        let null_id = HgId::null_id();
        // all local refs should be treated as bookmarks since Git does not write
        assert!(!filter.should_treat_local_ref_as_visible_head("main"));
        assert!(!filter.should_treat_local_ref_as_visible_head("foo"));
        // all remote names should be imported. Except */HEAD
        assert!(filter.should_import_remote_name("remote/main", null_id)?);
        assert!(filter.should_import_remote_name("remote/foo", null_id)?);
        assert!(filter.should_import_remote_name("upstream/bar", null_id)?);
        assert!(!filter.should_import_remote_name("upstream/HEAD", null_id)?);
        // detects main branch
        assert!(filter.is_main_remote_name("remote/b1"));
        assert!(!filter.is_main_remote_name("remote/b2"));
        Ok(())
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
