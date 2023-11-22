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
use crate::UnionMatcher;

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

    warnings: Vec<String>,
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

    /// Initialize HintedMatcher from normalized patterns, deriving hints.
    pub(crate) fn from_patterns(
        // Already normalized patterns. `None` means user specified zero patterns.
        // `Some(&[])` means there were zero patterns after normalization.
        pats: Option<&[Pattern]>,
        // Pre-expanded filesets from Python. `None` means no filesets were
        // specified. `Some(&[])` means there were filesets, but they evaluated
        // to an empty set of files.
        fs: Option<&[RepoPathBuf]>,
        empty_means_always_match: bool,
        case_sensitive: bool,
    ) -> Result<Self> {
        let pats_none = pats.is_none();
        let fs_none = fs.is_none();

        let pats = pats.unwrap_or_default();
        let fs = fs.unwrap_or_default();

        // Handle the always/never cases first since they are subtle.
        let (always, never) = if pats_none && fs_none {
            (empty_means_always_match, !empty_means_always_match)
        } else if pats.is_empty() && fs.is_empty() {
            (false, true)
        } else {
            let always = pats
                .iter()
                .any(|p| p.pattern == "**" && (p.kind.is_path() || p.kind.is_glob()));
            (always, false)
        };

        if always || never {
            return Ok(Self {
                case_sensitive,
                matcher: if always {
                    Arc::new(AlwaysMatcher::new())
                } else {
                    Arc::new(NeverMatcher::new())
                },
                always_matches: always,
                never_matches: never,
                all_recursive_paths: true,
                exact_files: Vec::new(),
                warnings: Vec::new(),
            });
        }

        // Now we can be sure at least one of pats or fileset is non-empty.

        let mut matchers: Vec<DynMatcher> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();

        if !pats.is_empty() {
            let (m, w) = build_matcher_from_patterns(pats, case_sensitive)?;
            matchers.push(m);
            warnings.extend(w);
        }

        if !fs.is_empty() {
            matchers.push(Arc::new(ExactMatcher::new(fs.iter(), case_sensitive)));
        }

        assert!(!matchers.is_empty());

        Ok(Self {
            case_sensitive,
            matcher: Arc::new(UnionMatcher::new_or_single(matchers)),
            always_matches: false,
            never_matches: false,
            all_recursive_paths: fs.is_empty()
                && pats
                    .iter()
                    .all(|p| p.kind.is_path() && p.kind.is_recursive()),
            exact_files: pats.iter().filter_map(|p| p.exact_file.clone()).collect(),
            warnings,
        })
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
            warnings: self
                .warnings
                .iter()
                .chain(other.warnings.iter())
                .cloned()
                .collect(),
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
            warnings: self
                .warnings
                .iter()
                .chain(other.warnings.iter())
                .cloned()
                .collect(),
        }
    }

    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    pub(crate) fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings.extend(warnings);
        self
    }

    pub fn matcher(&self) -> &DynMatcher {
        &self.matcher
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::pattern::normalize_patterns;
    use crate::PatternKind;

    #[test]
    fn test_empty_hinted_matcher() -> Result<()> {
        let always = HintedMatcher::from_patterns(None, None, true, true)?;
        assert!(always.always_matches());
        assert!(!always.never_matches());
        assert!(always.matches_file("foo/bar".try_into()?)?);

        let never = HintedMatcher::from_patterns(None, None, false, true)?;
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
    fn test_pattern_combinations() -> Result<()> {
        let pat = vec![Pattern::new(PatternKind::Path, "foo".to_string())];
        let fileset: Vec<RepoPathBuf> = vec!["foo".to_string().try_into()?];

        for pat in [None, Some(&pat[..0]), Some(&pat)] {
            for fileset in [None, Some(&fileset[..0]), Some(&fileset)] {
                for empty_means_always_match in [true, false] {
                    let assert_context = format!(
                        "pat={:?} fileset={:?} empty_means_always={:?}",
                        pat, fileset, empty_means_always_match
                    );

                    let m =
                        HintedMatcher::from_patterns(pat, fileset, empty_means_always_match, true)?;

                    let should_always_match =
                        pat.is_none() && fileset.is_none() && empty_means_always_match;

                    assert_eq!(m.always_matches(), should_always_match, "{assert_context}");
                    assert_eq!(
                        m.matches_file("a/b/c".try_into()?)?,
                        should_always_match,
                        "{assert_context}"
                    );

                    let should_never_match = match (pat, fileset) {
                        (None, None) => !empty_means_always_match,
                        (Some(p), None) => p.is_empty(),
                        (None, Some(f)) => f.is_empty(),
                        (Some(p), Some(f)) => p.is_empty() && f.is_empty(),
                    };
                    assert_eq!(m.never_matches(), should_never_match, "{assert_context}",);
                    assert_eq!(
                        m.matches_file("foo".try_into()?)?,
                        !should_never_match,
                        "{assert_context}"
                    );

                    if should_always_match || should_never_match {
                        assert!(m.exact_files().is_empty(), "{assert_context}");
                    }

                    let should_be_all_recursive_paths = should_always_match
                        || should_never_match
                        || fileset.map_or(true, |fs| fs.is_empty());
                    assert_eq!(
                        m.all_recursive_paths(),
                        should_be_all_recursive_paths,
                        "{assert_context}"
                    );
                }
            }
        }

        Ok(())
    }

    #[test]
    fn test_always_matches() -> Result<()> {
        let matcher = HintedMatcher::from_patterns(
            Some(
                &normalize_patterns(
                    &[".", "doesnt-matter"],
                    PatternKind::RelPath,
                    "/root".as_ref(),
                    "/root".as_ref(),
                    false,
                    Some(&mut std::io::empty()),
                )?
                .0,
            ),
            None,
            true,
            true,
        )?;

        assert!(matcher.always_matches());
        assert!(!matcher.never_matches());
        assert!(matcher.all_recursive_paths());
        assert!(matcher.exact_files().is_empty());

        Ok(())
    }

    #[test]
    fn test_exact_files() -> Result<()> {
        let foo_dot_c = Pattern::new(PatternKind::Path, "foo.c".into())
            .with_exact_file(Some("foo.c".to_string().try_into()?));
        let full_glob = Pattern::new(PatternKind::Glob, "**".into());

        let pats = HintedMatcher::from_patterns(Some(&[foo_dot_c]), None, true, true)?;
        let exclude = HintedMatcher::from_patterns(Some(&[full_glob]), None, true, true)?;

        let m = pats.exclude(&exclude);
        assert!(!m.always_matches());
        assert!(!m.never_matches());

        // Make sure we still match foo.c even though the exclude matches everything.
        assert!(m.matches_file("foo.c".try_into()?)?);

        Ok(())
    }

    #[test]
    fn test_filesets() -> Result<()> {
        let foo_dot_c = Pattern::new(PatternKind::Path, "foo.c".into());

        let m = HintedMatcher::from_patterns(
            Some(&[foo_dot_c]),
            Some(&["foo/bar".to_string().try_into()?]),
            true,
            true,
        )?;

        assert!(!m.all_recursive_paths());
        assert!(m.matches_file("foo.c".try_into()?)?);
        assert!(m.matches_file("foo/bar".try_into()?)?);

        let m = HintedMatcher::from_patterns(
            Some(&[]),
            Some(&["foo/bar".to_string().try_into()?]),
            true,
            true,
        )?;
        assert!(!m.never_matches());
        assert!(!m.always_matches());
        assert!(m.matches_file("foo/bar".try_into()?)?);

        Ok(())
    }
}
