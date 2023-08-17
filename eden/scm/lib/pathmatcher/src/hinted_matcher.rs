/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use types::RepoPath;
use types::RepoPathBuf;

use crate::matcher::build_matcher_from_patterns;
use crate::pattern::Pattern;
use crate::AlwaysMatcher;
use crate::DifferenceMatcher;
use crate::DirectoryMatch;
use crate::DynMatcher;
use crate::ExactMatcher;
use crate::IntersectMatcher;
use crate::Matcher;
use crate::NeverMatcher;

/// HintedMatcher tracks some basic metadata about the patterns in
/// order to fulfill Python's matcher interface. The hints are best
/// effort, and are really only "designed" to handle specific cases.
#[derive(Clone)]
pub struct HintedMatcher {
    matcher: DynMatcher,
    case_sensitive: bool,

    // These "hints" are used by the Python matcher interface. Except for
    // `exact_files`, they are only used for performance optimizations.
    exact_files: Vec<RepoPathBuf>,
    always_matches: bool,
    never_matches: bool,
    all_recursive_paths: bool,
}

impl Matcher for HintedMatcher {
    fn matches_directory(&self, path: &RepoPath) -> Result<DirectoryMatch> {
        self.matcher.matches_directory(path)
    }

    fn matches_file(&self, path: &RepoPath) -> Result<bool> {
        self.matcher.matches_file(path)
    }
}

impl HintedMatcher {
    pub fn always_matches(&self) -> bool {
        self.always_matches
    }

    pub fn never_matches(&self) -> bool {
        self.never_matches
    }

    pub fn all_recursive_paths(&self) -> bool {
        self.all_recursive_paths
    }

    pub fn exact_files(&self) -> &[RepoPathBuf] {
        &self.exact_files
    }

    /// Initialize HintedMatcher, deriving hints from given patterns.
    pub(crate) fn from_patterns(
        pats: &[Pattern],
        empty_means_always_match: bool,
        case_sensitive: bool,
    ) -> Result<Self> {
        if pats.is_empty() {
            Ok(Self {
                case_sensitive,
                matcher: if empty_means_always_match {
                    Arc::new(AlwaysMatcher::new())
                } else {
                    Arc::new(NeverMatcher::new())
                },
                exact_files: Vec::new(),
                always_matches: empty_means_always_match,
                never_matches: !empty_means_always_match,
                all_recursive_paths: false,
            })
        } else {
            Ok(Self {
                case_sensitive,
                matcher: build_matcher_from_patterns(pats, case_sensitive)?,
                exact_files: pats.iter().filter_map(|p| p.exact_file.clone()).collect(),
                // This is so we can mark "sl log ." as an always() matcher, enabling
                // various Python fast paths. ("." AKA "relpath:." is normalized to "" when run
                // from repo root.)
                always_matches: pats
                    .iter()
                    .any(|p| p.pattern.is_empty() && p.kind.is_path() && p.kind.is_recursive()),
                never_matches: false,
                all_recursive_paths: pats
                    .iter()
                    .all(|p| p.kind.is_path() && p.kind.is_recursive()),
            })
        }
    }

    pub fn include(&self, other: &Self) -> Self {
        if other.always_matches {
            return self.clone();
        }

        Self {
            matcher: Arc::new(IntersectMatcher::new(vec![
                self.matcher.clone(),
                other.matcher.clone(),
            ])),
            exact_files: self.exact_files.clone(),
            always_matches: self.always_matches && other.always_matches,
            never_matches: self.never_matches || other.never_matches,
            all_recursive_paths: self.all_recursive_paths && other.always_matches,
            case_sensitive: self.case_sensitive,
        }
    }

    pub fn exclude(&self, other: &Self) -> Self {
        if other.never_matches {
            return self.clone();
        }

        let mut other_matcher = other.matcher.clone();

        // Exact files in positional patterns override -X exclusion. For example, "sl
        // status file.c -Xpath:." will still show "file.c" even though "path:." matches
        // everything. Note that exact files don't seem to override -I patterns.
        if !self.exact_files.is_empty() {
            other_matcher = Arc::new(DifferenceMatcher::new(
                other_matcher,
                ExactMatcher::new(self.exact_files.iter(), self.case_sensitive),
            ));
        }

        Self {
            matcher: Arc::new(DifferenceMatcher::new(self.matcher.clone(), other_matcher)),
            exact_files: self.exact_files.clone(),
            always_matches: self.always_matches && other.never_matches,
            never_matches: self.never_matches
                || (other.always_matches && self.exact_files.is_empty()),
            all_recursive_paths: self.all_recursive_paths && other.never_matches,
            case_sensitive: self.case_sensitive,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::PatternKind;

    #[test]
    fn test_empty_hinted_matcher() -> Result<()> {
        let always = HintedMatcher::from_patterns(&[], true, true)?;
        assert!(always.always_matches());
        assert!(!always.never_matches());
        assert!(always.matches_file("foo/bar".try_into()?)?);

        let never = HintedMatcher::from_patterns(&[], false, true)?;
        assert!(never.never_matches());
        assert!(!never.always_matches());
        assert!(!never.matches_file("foo/bar".try_into()?)?);

        assert!(always.include(&never).never_matches());
        assert!(!always.include(&never).always_matches());

        assert!(always.exclude(&never).always_matches());
        assert!(!always.exclude(&never).never_matches());

        assert!(always.include(&always).always_matches());
        assert!(!always.include(&always).never_matches());

        assert!(always.exclude(&always).never_matches());
        assert!(!always.exclude(&always).always_matches());

        assert!(never.include(&never).never_matches());
        assert!(!never.include(&never).always_matches());

        assert!(never.exclude(&never).never_matches());
        assert!(!never.exclude(&never).always_matches());

        assert!(never.exclude(&always).never_matches());
        assert!(!never.exclude(&always).always_matches());

        assert!(never.include(&always).never_matches());
        assert!(!never.include(&always).always_matches());

        Ok(())
    }

    #[test]
    fn test_exact_files() -> Result<()> {
        let foo_dot_c = Pattern::new(PatternKind::Path, "foo.c".into())
            .with_exact_file(Some("foo.c".to_string().try_into()?));
        let full_glob = Pattern::new(PatternKind::Glob, "**".into());

        let pats = HintedMatcher::from_patterns(&[foo_dot_c], true, true)?;
        let exclude = HintedMatcher::from_patterns(&[full_glob], true, true)?;

        let m = pats.exclude(&exclude);
        assert!(!m.always_matches());
        assert!(!m.never_matches());

        // Make sure we still match foo.c even though the exclude matches everything.
        assert!(m.matches_file("foo.c".try_into()?)?);

        Ok(())
    }
}
