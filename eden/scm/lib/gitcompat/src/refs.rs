/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt;

use anyhow::Result;
use fs_err as fs;
use pathmatcher_types::AlwaysMatcher;
use pathmatcher_types::DirectoryMatch;
use pathmatcher_types::Matcher;
use types::HgId;
use types::RepoPath;

use crate::rungit::BareGit;

/// Value of a Git reference.
#[derive(Clone)]
pub enum ReferenceValue {
    /// Symbolic link.
    Sym(String),
    /// SHA1 object id.
    Id(HgId),
}

/// Ref name -> value
type ReferenceMap = BTreeMap<String, ReferenceValue>;

// This is a macro, not a function, because it uses "return".
macro_rules! return_ok_if_not_found {
    ($expr:expr) => {{
        match $expr {
            Err(e) if e.kind() == ::std::io::ErrorKind::NotFound => return Ok(()),
            v => v,
        }
    }};
}

impl BareGit {
    /// Resolve the hash of Git "HEAD", aka, ".".
    pub fn resolve_head(&self) -> Result<HgId> {
        let id = self
            .lookup_reference_follow_links("HEAD")?
            .unwrap_or_else(|| *HgId::null_id());
        Ok(id)
    }

    /// Lookup a reference by full name like "refs/heads/main".
    /// Returns `None` if the reference does not exist.
    pub fn lookup_reference(&self, name: &str) -> Result<Option<ReferenceValue>> {
        let mut result = None;
        // Access to "result.is_empty()" without offending borrowck.
        let has_result = Cell::new(false);
        let insert = &mut |n, v| {
            if n == name {
                has_result.set(true);
                result = Some(v);
            }
        };
        let matcher = AlwaysMatcher::new();
        self.populate_loose_file_reference(&matcher, Cow::Borrowed(name), insert)?;
        if !has_result.get() {
            self.populate_packed_references(&matcher, insert)?;
        }
        Ok(result)
    }

    /// Lookup a reference by full name like "refs/heads/main". Follow symlinks to resolve
    /// to an object id.
    ///
    /// Returns `None` if the reference or its referred reference does not exist.
    /// For example, a newly created Git repo will have `HEAD` pointing to `refs/heads/main`,
    /// but the `refs/heads/main` does not exist.
    pub fn lookup_reference_follow_links(&self, name: &str) -> Result<Option<HgId>> {
        let mut value = self.lookup_reference(name)?;
        loop {
            match value {
                // NOTE: This does not yet check circular references.
                Some(ReferenceValue::Sym(target)) => value = self.lookup_reference(&target)?,
                Some(ReferenceValue::Id(id)) => return Ok(Some(id)),
                None => return Ok(None),
            }
        }
    }

    /// Read and list Git references.
    ///
    /// If `matcher` is specified, it can be used to filter out uninteresting references
    /// like tags, remote references, eetc.
    ///
    /// Calling this function will re-read references from disk. There is no caching
    /// at this layer.
    pub fn list_references(&self, matcher: Option<&dyn Matcher>) -> Result<ReferenceMap> {
        let default_matcher;
        let matcher = match matcher {
            None => {
                default_matcher = AlwaysMatcher::new();
                &default_matcher
            }
            Some(v) => v,
        };
        // The order matters. Loose entries can override packed entries. So read loose last.
        let mut result = ReferenceMap::default();
        let insert = &mut |k, v| {
            result.insert(k, v);
        };
        self.populate_packed_references(matcher, insert)?;
        self.populate_loose_directory_references(matcher, "refs", insert)?;
        for name in ["HEAD", "FETCH_HEAD", "MERGE_HEAD"] {
            self.populate_loose_file_reference(matcher, Cow::Borrowed(name), insert)?;
        }
        Ok(result)
    }
}

// Implementation details used by list_references().
impl BareGit {
    fn populate_loose_file_reference(
        &self,
        matcher: &dyn Matcher,
        name: Cow<str>,
        insert: &mut dyn FnMut(String, ReferenceValue),
    ) -> Result<()> {
        if !matcher.matches_file(RepoPath::from_str(name.as_ref())?)? {
            return Ok(());
        }
        let path = self.git_dir().join(name.as_ref());
        let content = return_ok_if_not_found!(fs::read_to_string(path))?;
        let value = ReferenceValue::from_content(&content)?;
        insert(name.into_owned(), value);
        Ok(())
    }

    fn populate_loose_directory_references(
        &self,
        matcher: &dyn Matcher,
        prefix: &str,
        insert: &mut dyn FnMut(String, ReferenceValue),
    ) -> Result<()> {
        if let DirectoryMatch::Nothing = matcher.matches_directory(RepoPath::from_str(prefix)?)? {
            return Ok(());
        }
        let dir = return_ok_if_not_found!(fs::read_dir(self.git_dir().join(prefix)))?;
        for entry in dir {
            let entry = entry?;
            let file_name = match entry.file_name().into_string() {
                Ok(s) if s != "." && s != ".." => s,
                // Ignore non-utf8 names.
                _ => continue,
            };
            let name = format!("{}/{}", prefix, file_name);
            let file_type = entry.file_type()?;
            if file_type.is_dir() {
                self.populate_loose_directory_references(matcher, &name, insert)?;
            } else {
                self.populate_loose_file_reference(matcher, Cow::Owned(name), insert)?;
            }
        }

        Ok(())
    }

    fn populate_packed_references(
        &self,
        matcher: &dyn Matcher,
        insert: &mut dyn FnMut(String, ReferenceValue),
    ) -> Result<()> {
        let content =
            return_ok_if_not_found!(fs::read_to_string(self.git_dir().join("packed-refs")))?;

        // To support "peeled" refs.
        let mut last_inserted_name: Option<&str> = None;

        for line in content.lines() {
            if line.starts_with('#') {
                // Header, like "# pack-refs with: peeled fully-peeled sorted".
                continue;
            } else if let Some(rest) = line.strip_prefix('^') {
                // peeled ref. Example:
                //
                //   0e49e712b019f3d6685503d4f79b66f24f178757 refs/tags/foo
                //   ^cfd9b8592ff5454285650179a3e8d086481b4921
                //
                // 0e49 is the annotated tag object, cfd9 is the (peeled) commit object.
                if let Some(last_name) = last_inserted_name {
                    let id = HgId::from_hex(rest.as_bytes())?;
                    let value = ReferenceValue::Id(id);
                    insert(last_name.to_owned(), value);
                }
            } else if let Some((hex, name)) = line.split_once(' ') {
                if !matcher.matches_file(RepoPath::from_str(name)?)? {
                    // This ref is filtered out. Ignore the next peeled line.
                    last_inserted_name = None;
                    continue;
                }
                let id = HgId::from_hex(hex.as_bytes())?;
                let value = ReferenceValue::Id(id);
                insert(name.to_owned(), value);
                last_inserted_name = Some(name);
            }
        }

        Ok(())
    }
}

impl ReferenceValue {
    fn from_content(content: &str) -> Result<Self> {
        let content = content.trim_end();
        let result = match content.strip_prefix("ref: ") {
            Some(rest) => Self::Sym(rest.to_string()),
            None => Self::Id(HgId::from_hex(content.as_bytes())?),
        };
        Ok(result)
    }
}

impl fmt::Debug for ReferenceValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ReferenceValue::Sym(v) => {
                f.write_str("=> ")?;
                f.write_str(v)
            }
            ReferenceValue::Id(v) => f.write_str(&v.to_hex()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::TempDir;

    use super::*;

    impl BareGit {
        fn debug_list_references(&self, matcher: Option<&dyn Matcher>) -> Vec<String> {
            match self.list_references(matcher) {
                Ok(refs) => refs
                    .into_iter()
                    .map(|(k, v)| format!("{} {:?}", k, v))
                    .collect(),
                Err(e) => vec![e.to_string()],
            }
        }

        // Show both raw value and symlink target
        fn debug_lookup_reference(&self, name: &str) -> String {
            let value = self.lookup_reference(name).unwrap();
            let resolved_id = self.lookup_reference_follow_links(name).unwrap();
            match value {
                Some(ReferenceValue::Id(value_id)) => {
                    assert_eq!(Some(value_id), resolved_id);
                    value_id.to_hex()
                }
                Some(ReferenceValue::Sym(name)) => {
                    let resolved = match resolved_id {
                        Some(id) => id.to_hex(),
                        None => "None".to_owned(),
                    };
                    format!("{} => {}", name, resolved)
                }
                None => "None".to_owned(),
            }
        }
    }

    #[test]
    fn test_references_nothing() {
        let (dir, git) = setup(&[], None);
        assert!(git.list_references(None).unwrap().is_empty());
        drop(dir);
    }

    #[test]
    fn test_references_mixed() {
        let (_dir, git) = setup(
            &[
                // dangling symlink will be preserved
                "HEAD ref: refs/heads/main",
                // overrides the "packed" version.
                "refs/heads/foo 2222222222222222222222222222222222222222",
                "refs/heads/bar ref: refs/tags/v4",
                "refs/tags/v1 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "refs/tags/v2 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            ],
            Some(concat!(
                "# pack-refs with: peeled fully-peeled sorted\n",
                "3333333333333333333333333333333333333333 refs/remotes/origin/main\n",
                "4444444444444444444444444444444444444444 refs/remotes/origin/dev\n",
                "1111111111111111111111111111111111111110 refs/heads/foo\n",
                "9999999999999999999999999999999999999999 refs/tags/v3\n",
                "^cccccccccccccccccccccccccccccccccccccccc\n",
                "999999999999999999999999999999999999999a refs/tags/v4\n",
                "^dddddddddddddddddddddddddddddddddddddddd\n",
            )),
        );
        assert_eq!(
            git.debug_list_references(None),
            [
                "HEAD => refs/heads/main",
                "refs/heads/bar => refs/tags/v4",
                "refs/heads/foo 2222222222222222222222222222222222222222",
                "refs/remotes/origin/dev 4444444444444444444444444444444444444444",
                "refs/remotes/origin/main 3333333333333333333333333333333333333333",
                "refs/tags/v1 aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "refs/tags/v2 bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "refs/tags/v3 cccccccccccccccccccccccccccccccccccccccc",
                "refs/tags/v4 dddddddddddddddddddddddddddddddddddddddd"
            ]
        );

        // Test filtering out tags. The peeled hashes ("cc" and "dd") should not affect other entries.
        struct NoTags;
        impl Matcher for NoTags {
            fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch> {
                let v = match path.starts_with(RepoPath::from_str("refs/tags")?, true) {
                    true => DirectoryMatch::Nothing,
                    false => DirectoryMatch::ShouldTraverse,
                };
                Ok(v)
            }

            fn matches_file(&self, path: &RepoPath) -> Result<bool> {
                Ok(!path.starts_with(RepoPath::from_str("refs/tags")?, true))
            }
        }
        assert_eq!(
            git.debug_list_references(Some(&NoTags)),
            [
                "HEAD => refs/heads/main",
                "refs/heads/bar => refs/tags/v4",
                "refs/heads/foo 2222222222222222222222222222222222222222",
                "refs/remotes/origin/dev 4444444444444444444444444444444444444444",
                "refs/remotes/origin/main 3333333333333333333333333333333333333333"
            ]
        );

        // Test lookup_reference
        // from loose files
        assert_eq!(
            git.debug_lookup_reference("refs/tags/v1"),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        // from packed refs
        assert_eq!(
            git.debug_lookup_reference("refs/tags/v3"),
            "cccccccccccccccccccccccccccccccccccccccc"
        );
        // from loose files, ignore conflicting packed refs
        assert_eq!(
            git.debug_lookup_reference("refs/heads/foo"),
            "2222222222222222222222222222222222222222"
        );
        // dancling symlink
        assert_eq!(
            git.debug_lookup_reference("HEAD"),
            "refs/heads/main => None"
        );
        // follow symlink and peel
        assert_eq!(
            git.debug_lookup_reference("refs/heads/bar"),
            "refs/tags/v4 => dddddddddddddddddddddddddddddddddddddddd"
        );
        // not found
        assert_eq!(
            git.debug_lookup_reference("refs/not-found/not-found"),
            "None"
        );
    }

    fn setup(loose: &[&str], packed: Option<&str>) -> (TempDir, BareGit) {
        let dir = tempfile::tempdir().unwrap();
        for entry in loose {
            let (name, value) = entry.split_once(' ').unwrap();
            let path = dir.path().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, value).unwrap();
        }
        if let Some(data) = packed {
            let path = dir.path().join("packed-refs");
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, data).unwrap();
        }
        let config: BTreeMap<&str, &str> = BTreeMap::new();
        let git_dir = dir.path().to_owned();
        (dir, BareGit::from_git_dir_and_config(git_dir, &config))
    }
}
