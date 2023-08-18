/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use types::RepoPathBuf;

use crate::pattern::normalize_patterns;
use crate::pattern::Pattern;
use crate::AlwaysMatcher;
use crate::DifferenceMatcher;
use crate::DynMatcher;
use crate::Error;
use crate::HintedMatcher;
use crate::IntersectMatcher;
use crate::PatternKind;
use crate::RegexMatcher;
use crate::TreeMatcher;
use crate::UnionMatcher;

/// Create top level matcher from non-normalized CLI input.
pub fn cli_matcher(
    patterns: &[String],
    include: &[String],
    exclude: &[String],
    default_pattern_type: PatternKind,
    case_sensitive: bool,
    root: &Path,
    cwd: &Path,
) -> Result<HintedMatcher> {
    cli_matcher_with_filesets(
        patterns,
        None,
        include,
        None,
        exclude,
        None,
        default_pattern_type,
        case_sensitive,
        root,
        cwd,
    )
}

/// Create top level matcher from non-normalized CLI input.
pub fn cli_matcher_with_filesets(
    patterns: &[String],
    patterns_filesets: Option<&[RepoPathBuf]>,
    include: &[String],
    include_filesets: Option<&[RepoPathBuf]>,
    exclude: &[String],
    exclude_filesets: Option<&[RepoPathBuf]>,
    default_pattern_type: PatternKind,
    case_sensitive: bool,
    root: &Path,
    cwd: &Path,
) -> Result<HintedMatcher> {
    let pattern_matcher = HintedMatcher::from_patterns(
        &normalize_patterns(patterns, default_pattern_type, root, cwd, false)?,
        patterns_filesets,
        true,
        case_sensitive,
    )?;

    let include_matcher = HintedMatcher::from_patterns(
        &normalize_patterns(include, PatternKind::Glob, root, cwd, true)?,
        include_filesets,
        true,
        case_sensitive,
    )?;

    let exclude_matcher = HintedMatcher::from_patterns(
        &normalize_patterns(exclude, PatternKind::Glob, root, cwd, true)?,
        exclude_filesets,
        false,
        case_sensitive,
    )?;

    Ok(pattern_matcher
        .include(&include_matcher)
        .exclude(&exclude_matcher))
}

/// Build matcher from normalized patterns.
///
/// The relationship between `patterns`, `include` and `exclude` is like:
///     (patterns & include) - exclude
///
/// `patterns`, `include`, `exclude` can be empty:
///   * If `patterns` is empty, we will build a AlwaysMatcher
///   * If `include` is empty, it will be ignored
///   * If `exclude` is empty it will be ignored
pub fn build_matcher(
    patterns: &[Pattern],
    include: &[Pattern],
    exclude: &[Pattern],
    case_sensitive: bool,
) -> Result<DynMatcher> {
    let mut m: DynMatcher = if patterns.is_empty() {
        Arc::new(AlwaysMatcher::new())
    } else {
        build_matcher_from_patterns(patterns, case_sensitive)?
    };

    if !include.is_empty() {
        let im = build_matcher_from_patterns(include, case_sensitive)?;
        m = Arc::new(IntersectMatcher::new(vec![m, im]));
    }

    if !exclude.is_empty() {
        let em = build_matcher_from_patterns(exclude, case_sensitive)?;
        m = Arc::new(DifferenceMatcher::new(m, em));
    }

    Ok(m)
}

pub(crate) fn build_matcher_from_patterns(
    patterns: &[Pattern],
    case_sensitive: bool,
) -> Result<DynMatcher> {
    assert!(!patterns.is_empty(), "patterns should not be empty");

    let mut matchers: Vec<DynMatcher> = Vec::new();

    let grouped_patterns = group_by_pattern_kind(patterns);
    for (kind, pats) in &grouped_patterns {
        let m: DynMatcher = if kind.is_glob() || kind.is_path() {
            Arc::new(TreeMatcher::from_rules(pats.iter(), case_sensitive)?)
        } else if kind.is_regex() {
            let regex_pat = format!("(?:{})", pats.join("|"));
            Arc::new(RegexMatcher::new(&regex_pat, case_sensitive)?)
        } else {
            return Err(Error::UnsupportedPatternKind(kind.name().to_string()).into());
        };
        matchers.push(m);
    }

    if matchers.len() == 1 {
        Ok(matchers.remove(0))
    } else {
        Ok(Arc::new(UnionMatcher::new(matchers)))
    }
}

fn group_by_pattern_kind(patterns: &[Pattern]) -> HashMap<PatternKind, Vec<String>> {
    let mut res = HashMap::new();
    for p in patterns.iter() {
        res.entry(p.kind)
            .or_insert_with(Vec::new)
            .push(p.pattern.clone())
    }
    res
}

#[cfg(test)]
mod tests {
    use types::RepoPath;

    use super::*;
    use crate::DirectoryMatch;
    use crate::Matcher;

    macro_rules! path {
        ($s:expr) => {
            RepoPath::from_str($s).unwrap()
        };
    }

    #[test]
    fn test_build_matcher_with_all_empty() {
        // AlwaysMatcher
        let m = build_matcher(&[], &[], &[], true).unwrap();

        assert!(m.matches_file(path!("")).unwrap());
        assert!(m.matches_file(path!("a")).unwrap());
        assert!(m.matches_file(path!("a/b")).unwrap());
        assert!(m.matches_file(path!("z")).unwrap());
    }

    #[test]
    fn test_build_matcher_with_all_non_empty() {
        let patterns = &[Pattern::new(PatternKind::RE, r"a/t\d+.*\.py".to_string())];
        let include = &[Pattern::new(PatternKind::Glob, "a/t1*/**".to_string())];
        let exclude = &[Pattern::new(PatternKind::Glob, "a/t11/**".to_string())];

        let m = build_matcher(patterns, include, exclude, true).unwrap();

        assert_eq!(
            m.matches_directory(path!("a")).unwrap(),
            DirectoryMatch::ShouldTraverse
        );
        assert_eq!(
            m.matches_directory(path!("a/t1")).unwrap(),
            DirectoryMatch::ShouldTraverse
        );
        assert_eq!(
            m.matches_directory(path!("a/t11")).unwrap(),
            DirectoryMatch::Nothing
        );
        assert_eq!(
            m.matches_directory(path!("a/tt")).unwrap(),
            DirectoryMatch::Nothing
        );
        assert_eq!(
            m.matches_directory(path!("b")).unwrap(),
            DirectoryMatch::Nothing
        );

        assert!(m.matches_file(path!("a/t1/b.py")).unwrap());
        assert!(m.matches_file(path!("a/t12/b.py")).unwrap());
        assert!(!m.matches_file(path!("a/t11/b.py")).unwrap());
        assert!(!m.matches_file(path!("b/b.py")).unwrap());
    }

    #[test]
    fn test_build_matcher_with_empty_patterns() {
        let include = &[Pattern::new(PatternKind::Glob, "a/t1*/**".to_string())];
        let exclude = &[Pattern::new(PatternKind::Glob, "a/t11/**".to_string())];

        let m = build_matcher(&[], include, exclude, true).unwrap();

        assert_eq!(
            m.matches_directory(path!("a/t1a")).unwrap(),
            DirectoryMatch::Everything
        );
        assert_eq!(
            m.matches_directory(path!("a/t11")).unwrap(),
            DirectoryMatch::Nothing
        );
    }

    #[test]
    fn test_build_matcher_with_empty_include() {
        let patterns = &[Pattern::new(PatternKind::RE, r"a/t\d+.*\.py".to_string())];
        let exclude = &[Pattern::new(PatternKind::Glob, "a/t11/**".to_string())];

        let m = build_matcher(patterns, &[], exclude, true).unwrap();

        assert_eq!(
            m.matches_directory(path!("a/t1")).unwrap(),
            DirectoryMatch::ShouldTraverse
        );
        assert_eq!(
            m.matches_directory(path!("a/t11")).unwrap(),
            DirectoryMatch::Nothing
        );
    }

    #[test]
    fn test_build_matcher_with_empty_exclude() {
        let patterns = &[Pattern::new(PatternKind::RE, r"a/t\d+.*\.py".to_string())];
        let include = &[Pattern::new(PatternKind::Glob, "a/t1*/**".to_string())];

        let m = build_matcher(patterns, include, &[], true).unwrap();

        assert_eq!(
            m.matches_directory(path!("a/t1")).unwrap(),
            DirectoryMatch::ShouldTraverse
        );
        assert_eq!(
            m.matches_directory(path!("a/t11")).unwrap(),
            DirectoryMatch::ShouldTraverse
        );
    }

    #[test]
    fn test_cli_matcher_exact_precedence() -> Result<()> {
        let m = cli_matcher(
            &vec!["path:foo".to_string()],
            &[],
            &vec!["path:".to_string()],
            PatternKind::Glob,
            true,
            "/root".as_ref(),
            "/root/cwd".as_ref(),
        )?;

        assert!(m.matches_file(RepoPath::from_str("foo")?)?);

        Ok(())
    }
}
