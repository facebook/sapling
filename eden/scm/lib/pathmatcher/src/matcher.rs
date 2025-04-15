/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use anyhow::anyhow;
use types::RepoPathBuf;

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
use crate::pattern::Pattern;
use crate::pattern::explicit_pattern_kind;
use crate::pattern::normalize_patterns;
use crate::regex_matcher::SlowRegexMatcher;

/// Create top level matcher from non-normalized CLI input.
pub fn cli_matcher<R>(
    patterns: &[String],
    include: &[String],
    exclude: &[String],
    default_pattern_type: PatternKind,
    case_sensitive: bool,
    root: &Path,
    cwd: &Path,
    stdin: &mut R,
) -> Result<HintedMatcher>
where
    R: std::io::Read,
{
    // This expands relpath patterns as globs on Windows to emulate shell expansion.
    let patterns = expand_globs(patterns, default_pattern_type)?;
    let patterns = &patterns;

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
        stdin,
    )
}

/// Create top level matcher from non-normalized CLI input given
/// expanded filesets. The only intended external caller of this is
/// Python, where some processing has already happende (e.g. glob
/// expansion).
pub fn cli_matcher_with_filesets<R>(
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
    stdin: &mut R,
) -> Result<HintedMatcher>
where
    R: std::io::Read,
{
    let mut all_warnings = Vec::new();

    let mut normalize = |pats: &[_], default, force_recursive| -> Result<Option<Vec<Pattern>>> {
        if pats.is_empty() {
            Ok(None)
        } else {
            let (normalized, warnings) =
                normalize_patterns(pats, default, root, cwd, force_recursive, Some(stdin))?;

            all_warnings.extend(warnings);

            Ok(Some(normalized))
        }
    };

    let pattern_matcher = HintedMatcher::from_patterns(
        normalize(patterns, default_pattern_type, false)?.as_deref(),
        patterns_filesets,
        true,
        case_sensitive,
    )?;

    let include_matcher = HintedMatcher::from_patterns(
        normalize(include, PatternKind::Glob, true)?.as_deref(),
        include_filesets,
        true,
        case_sensitive,
    )?;

    let exclude_matcher = HintedMatcher::from_patterns(
        normalize(exclude, PatternKind::Glob, true)?.as_deref(),
        exclude_filesets,
        false,
        case_sensitive,
    )?;

    for fs in [&patterns_filesets, &include_filesets, &exclude_filesets] {
        if fs.map_or(false, |fs| fs.is_empty()) {
            // TODO: pipe the original fileset string to this warning
            all_warnings.push("fileset evaluated to zero files".to_string());
            break;
        }
    }

    Ok(pattern_matcher
        .include(&include_matcher)
        .exclude(&exclude_matcher)
        .with_warnings(all_warnings))
}

// "Manually" expand globs in relpath patterns on Windows.
// This emulates non-Windows shell expansion.
fn expand_globs(pats: &[String], default_pattern_type: PatternKind) -> Result<Vec<String>> {
    if !cfg!(windows) || default_pattern_type != PatternKind::RelPath {
        return Ok(pats.to_vec());
    }

    let mut expanded: Vec<String> = Vec::with_capacity(pats.len());
    for pat in pats {
        if explicit_pattern_kind(pat).is_some() {
            // Don't expand paths with explicit "kind". This includes "relpath:*foo".
            expanded.push(pat.clone());
        } else {
            match glob::glob(pat) {
                Ok(paths) => {
                    let mut globbed = paths
                        .map(|p| {
                            let p = p?;
                            p.to_str()
                                .map(|s| s.to_string())
                                .ok_or_else(|| anyhow!("invalid file path: {}", p.display()))
                        })
                        // Propagate permission errors.
                        .collect::<Result<Vec<_>>>()?;
                    if globbed.is_empty() {
                        // Glob didn't match any files - keep original pattern. AFAIK this serves two purposes:
                        //   1. Avoids "pats" becoming empty and accidentally matching everything.
                        //   2. Keeps "pat" in list of exact files so user will see warning "{pat}: no such file".
                        expanded.push(pat.clone());
                    } else {
                        expanded.append(&mut globbed);
                    }
                }
                // Invalid glob pattern - assume it wasn't a glob.
                Err(_) => expanded.push(pat.clone()),
            }
        }
    }
    Ok(expanded)
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
) -> Result<(DynMatcher, Vec<String>)> {
    let (mut m, mut warnings) = if patterns.is_empty() {
        (Arc::new(AlwaysMatcher::new()) as DynMatcher, Vec::new())
    } else {
        build_matcher_from_patterns(patterns, case_sensitive)?
    };

    if !include.is_empty() {
        let (im, w) = build_matcher_from_patterns(include, case_sensitive)?;
        m = Arc::new(IntersectMatcher::new(vec![m, im]));
        warnings.extend(w);
    }

    if !exclude.is_empty() {
        let (em, w) = build_matcher_from_patterns(exclude, case_sensitive)?;
        m = Arc::new(DifferenceMatcher::new(m, em));
        warnings.extend(w);
    }

    Ok((m, warnings))
}

pub(crate) fn build_matcher_from_patterns(
    patterns: &[Pattern],
    case_sensitive: bool,
) -> Result<(DynMatcher, Vec<String>)> {
    assert!(!patterns.is_empty(), "patterns should not be empty");

    let mut matchers: Vec<DynMatcher> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    let grouped_patterns = group_by_pattern_kind(patterns);
    for (kind, pats) in &grouped_patterns {
        let m: DynMatcher = if kind.is_glob() || kind.is_path() {
            Arc::new(TreeMatcher::from_rules(pats.iter(), case_sensitive)?)
        } else if kind.is_regex() {
            let regex_pat = format!("(?:{})", pats.join("|"));
            match RegexMatcher::new(&regex_pat, case_sensitive) {
                Ok(m) => Arc::new(m),
                // regex_automata doesn't export error introspection, so just try fancy on any error.
                Err(_) => {
                    let m = Arc::new(SlowRegexMatcher::new(&regex_pat, case_sensitive)?);
                    tracing::trace!(target: "pathmatcher_info", fancy_regex=true);
                    warnings.push("fancy regexes are deprecated and may stop working".to_string());
                    m
                }
            }
        } else {
            return Err(Error::UnsupportedPatternKind(kind.name().to_string()).into());
        };
        matchers.push(m);
    }

    Ok((UnionMatcher::new_or_single(matchers), warnings))
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
        let m = build_matcher(&[], &[], &[], true).unwrap().0;

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

        let m = build_matcher(patterns, include, exclude, true).unwrap().0;

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

        let m = build_matcher(&[], include, exclude, true).unwrap().0;

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

        let m = build_matcher(patterns, &[], exclude, true).unwrap().0;

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

        let m = build_matcher(patterns, include, &[], true).unwrap().0;

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
            &["path:foo".to_string()],
            &[],
            &["path:".to_string()],
            PatternKind::Glob,
            true,
            "/root".as_ref(),
            "/root/cwd".as_ref(),
            &mut std::io::empty(),
        )?;

        assert!(m.matches_file(RepoPath::from_str("foo")?)?);

        Ok(())
    }

    #[test]
    fn test_empty_listfile() -> Result<()> {
        let dir = tempfile::TempDir::new()?;

        let listfile = dir.path().join("listfile");
        fs_err::write(&listfile, "")?;

        let m = cli_matcher(
            &[format!("listfile:{}", listfile.to_str().unwrap())],
            &[],
            &[],
            PatternKind::Glob,
            true,
            "/root".as_ref(),
            "/root/cwd".as_ref(),
            &mut std::io::empty(),
        )?;

        assert!(!m.matches_file(RepoPath::from_str("foo")?)?);

        Ok(())
    }

    #[test]
    fn test_warnings() -> Result<()> {
        let dir = tempfile::TempDir::new()?;

        let listfile = dir.path().join("listfile");
        fs_err::write(&listfile, "")?;

        let m = cli_matcher_with_filesets(
            &[
                format!("listfile:{}", listfile.to_str().unwrap()),
                "foo*".to_string(),
            ],
            Some(&[]),
            &[],
            None,
            &[],
            None,
            PatternKind::RelPath,
            true,
            "/root".as_ref(),
            "/root/cwd".as_ref(),
            &mut std::io::empty(),
        )?;

        assert_eq!(
            m.warnings(),
            &[
                format!(
                    "empty listfile {} matches nothing",
                    listfile.to_str().unwrap()
                ),
                "possible glob in non-glob pattern 'foo*', did you mean 'glob:foo*'?".to_string(),
                "fileset evaluated to zero files".to_string(),
            ]
        );

        Ok(())
    }

    #[test]
    fn test_fancy_regex_compat() -> Result<()> {
        let m = cli_matcher(
            &["re:(?<!foo)bar".to_string()],
            &[],
            &[],
            PatternKind::RelPath,
            true,
            "/root".as_ref(),
            "/root/cwd".as_ref(),
            &mut std::io::empty(),
        )?;

        assert!(m.matches_file(path!("bar"))?);
        assert!(!m.matches_file(path!("foobar"))?);

        assert_eq!(
            m.warnings(),
            &["fancy regexes are deprecated and may stop working".to_string()]
        );

        Ok(())
    }

    #[test]
    fn test_expand_globs() -> Result<()> {
        let dir = tempfile::TempDir::new()?;

        let foo1 = dir.path().join("foo1");
        fs_err::write(&foo1, "")?;

        let foo2 = dir.path().join("foo2");
        fs_err::write(&foo2, "")?;

        let foo_glob = dir
            .path()
            .join("f*")
            .into_os_string()
            .into_string()
            .unwrap();

        let pats = &[
            format!("relpath:{}", foo_glob),
            "no".to_string(),
            foo_glob.clone(),
        ];

        let got = expand_globs(pats, PatternKind::RelPath)?;

        if cfg!(windows) {
            assert_eq!(
                got,
                vec![
                    // Not expanded - has explicit kind.
                    format!("relpath:{}", foo_glob),
                    // Not expanded - doesn't match any files.
                    "no".to_string(),
                    // Expanded into file names.
                    foo1.into_os_string().into_string().unwrap(),
                    foo2.into_os_string().into_string().unwrap(),
                ]
            );
        } else {
            assert_eq!(got, pats.to_vec());
        }

        let got = expand_globs(pats, PatternKind::Glob)?;
        assert_eq!(got, pats.to_vec());

        Ok(())
    }
}
