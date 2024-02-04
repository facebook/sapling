/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::str::FromStr;

use anyhow::Result;
use types::RepoPathBuf;

use crate::error::Error;
use crate::expand_curly_brackets;
use crate::normalize_glob;
use crate::plain_to_glob;
use crate::utils::first_glob_operator_index;
use crate::utils::make_glob_recursive;

#[derive(Debug, PartialEq, Copy, Clone, Hash, Eq)]
pub enum PatternKind {
    /// a regular expression relative to repository root, check [RegexMatcher]
    /// for supported RE syntax
    RE,

    /// a shell-style glob pattern relative to cwd
    Glob,

    /// a path relative to the repository root, and when the path points to a
    /// directory, it is matched recursively
    Path,

    /// an unrooted glob (e.g.: *.c matches C files in all dirs)
    RelGlob,

    /// a path relative to cwd
    RelPath,

    /// an unrooted regular expression, needn't match the start of a path
    RelRE,

    /// read file patterns per line from a file
    ListFile,

    /// read file patterns with null byte delimiters from a file
    ListFile0,

    /// a fileset expression
    Set,

    /// a path relative to repository root, which is matched non-recursively (will
    /// not match subdirectories)
    RootFilesIn,
}

impl PatternKind {
    pub fn name(&self) -> &'static str {
        match self {
            PatternKind::RE => "re",
            PatternKind::Glob => "glob",
            PatternKind::Path => "path",
            PatternKind::RelGlob => "relglob",
            PatternKind::RelPath => "relpath",
            PatternKind::RelRE => "relre",
            PatternKind::ListFile => "listfile",
            PatternKind::ListFile0 => "listfile0",
            PatternKind::Set => "set",
            PatternKind::RootFilesIn => "rootfilesin",
        }
    }

    pub fn is_glob(&self) -> bool {
        matches!(self, Self::Glob | Self::RelGlob)
    }

    pub fn is_path(&self) -> bool {
        matches!(self, Self::Path | Self::RelPath | Self::RootFilesIn)
    }

    pub fn is_regex(&self) -> bool {
        matches!(self, Self::RE | Self::RelRE)
    }

    pub fn is_recursive(&self) -> bool {
        matches!(self, Self::Path | Self::RelPath)
    }

    pub fn is_cwd_relative(&self) -> bool {
        matches!(self, Self::RelPath | Self::Glob)
    }

    pub fn is_free(&self) -> bool {
        matches!(self, Self::RelGlob | Self::RelRE)
    }
}

impl std::str::FromStr for PatternKind {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "re" => Ok(PatternKind::RE),
            "glob" => Ok(PatternKind::Glob),
            "path" => Ok(PatternKind::Path),
            "relglob" => Ok(PatternKind::RelGlob),
            "relpath" => Ok(PatternKind::RelPath),
            "relre" => Ok(PatternKind::RelRE),
            "listfile" => Ok(PatternKind::ListFile),
            "listfile0" => Ok(PatternKind::ListFile0),
            "set" => Ok(PatternKind::Set),
            "rootfilesin" => Ok(PatternKind::RootFilesIn),
            _ => Err(Error::UnsupportedPatternKind(s.to_string())),
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct Pattern {
    pub(crate) kind: PatternKind,
    pub(crate) pattern: String,
    pub(crate) source: Option<String>,

    // Any "exact" file name implied by this pattern. For example, "sl status
    // foo" has pattern "relpath:foo" which implies exact file "foo" (even
    // though the pattern matches "foo/**"). The exact file is used for various
    // reasons in Python (see "match.files()" calls).
    pub(crate) exact_file: Option<RepoPathBuf>,
}

impl Pattern {
    pub(crate) fn new(kind: PatternKind, pattern: String) -> Self {
        Self {
            kind,
            pattern,
            source: None,
            exact_file: None,
        }
    }

    pub(crate) fn with_source(mut self, source: String) -> Self {
        self.source = Some(source);
        self
    }

    pub(crate) fn with_exact_file(mut self, exact: Option<RepoPathBuf>) -> Self {
        self.exact_file = exact;
        self
    }

    /// Build `Pattern` from str.
    ///
    /// * If the str doesn't have pattern kind prefix, we will use `default_kind`.
    /// * `source` is set to None.
    pub(crate) fn from_str(pattern: &str, default_kind: PatternKind) -> Self {
        let (kind, pat) = split_pattern(pattern, default_kind);
        Self {
            kind,
            pattern: pat.to_string(),
            source: None,
            exact_file: None,
        }
    }
}

/// Build `Pattern`s from strings. It calls `Pattern::from_str` to do actual work.
/// `patterns` must already be normalized.
pub fn build_patterns(patterns: &[String], default_kind: PatternKind) -> Vec<Pattern> {
    patterns
        .iter()
        .map(|s| Pattern::from_str(s, default_kind))
        .collect()
}

pub fn split_pattern<'a>(pat: &'a str, default_kind: PatternKind) -> (PatternKind, &'a str) {
    explicit_pattern_kind(pat).unwrap_or((default_kind, pat))
}

pub fn explicit_pattern_kind<'a>(pat: &'a str) -> Option<(PatternKind, &'a str)> {
    pat.split_once(':')
        .and_then(|(kind, pat)| Some((PatternKind::from_str(kind).ok()?, pat)))
}

// Normalize input patterns, also returning warnings for the user.
// All pattern kinds expand to globs except for regexes, which stay regexes.
// A pattern can expand to empty (e.g. empty "listfile").
#[tracing::instrument(level = "debug", skip(stdin), ret)]
pub(crate) fn normalize_patterns<I, R>(
    patterns: I,
    default_kind: PatternKind,
    root: &Path,
    cwd: &Path,
    force_recursive_glob: bool,
    // stdin should always be set at the root level, recursive calls
    // will recieve `None` as we only allow `listfile:-` at the root
    mut stdin: Option<&mut R>,
) -> Result<(Vec<Pattern>, Vec<String>)>
where
    I: IntoIterator + std::fmt::Debug,
    I::Item: AsRef<str> + std::fmt::Debug,
    R: std::io::Read,
{
    let mut result = Vec::new();
    let mut warnings = Vec::new();

    for pattern in patterns {
        let (kind, pat) = split_pattern(pattern.as_ref(), default_kind);

        // Expand curlies in globs (e.g. "foo/{bar/baz, qux}" to ["foo/bar/baz", "foo/qux"]).
        // Do this early so they aren't naively treated as paths. Note that we haven't
        // normalized "\" to "/", so a Windows path separator might be misinterpreted as a
        // curly escape. The alternative is to normalize "\" to "/" first, but that will
        // certainly break curly escapes.
        let pats = if kind.is_glob() {
            expand_curly_brackets(pat)
        } else {
            vec![pat.to_string()]
        };

        for mut pat in pats {
            // Normalize CWD-relative patterns to be relative to repo root.
            if kind.is_cwd_relative() {
                pat = match util::path::root_relative_path(root, cwd, pat.as_ref())? {
                    Some(pat) => pat
                        .into_os_string()
                        .into_string()
                        .map_err(|s| Error::NonUtf8(s.to_string_lossy().to_string()))?,
                    None => {
                        return Err(Error::PathOutsideRoot(
                            pat.to_string(),
                            root.to_string_lossy().to_string(),
                        )
                        .into());
                    }
                };
            }

            // Clean up path and normalize to "/" path separator.
            if kind.is_glob() || kind.is_path() {
                pat = normalize_path_pattern(&pat);

                // Path normalization yields "." for empty paths, which we don't want.
                if pat == "." {
                    pat = String::new();
                }
            }

            // This is the best moment to look for "exact" files. We have
            // expanded glob curlies, but haven't glob-escaped paths.
            let exact_file = exact_file(kind, &pat)?;
            tracing::trace!(?exact_file);

            // Escape glob characters so we can convert non-glob patterns into globs.
            if kind.is_path() {
                let escaped = plain_to_glob(&pat);
                if pat != escaped {
                    let pattern = pattern.as_ref();
                    warnings.push(format!(
                        "possible glob in non-glob pattern '{pattern}', did you mean 'glob:{pattern}'?"
                    ));
                    pat = escaped;
                }
            }

            // Make our loose globbing compatible with the tree matcher's strict globbing.
            if kind.is_glob() {
                pat = normalize_glob(&pat);
            }

            if kind.is_recursive() {
                pat = make_glob_recursive(&pat);
            }

            // This is to make "-I" and "-X" globs recursive by default.
            if force_recursive_glob && kind.is_glob() {
                pat = make_glob_recursive(&pat);
            }

            if kind.is_glob() && kind.is_free() {
                if !pat.is_empty() {
                    // relglob is unrooted, so give it a leading "**".
                    pat = format!("**/{pat}");
                }
            }

            // rootfilesin matches a directory non-recursively
            if kind == PatternKind::RootFilesIn {
                pat = if pat.is_empty() {
                    "*".to_string()
                } else {
                    format!("{pat}/*")
                };
            }

            if kind.is_regex() {
                let anchored = pat.starts_with('^');

                // "^" is not required - strip it.
                if anchored {
                    pat = pat[1..].to_string();
                }

                // relre without "^" needs leading ".*?" to become unanchored.
                if !anchored && kind.is_free() {
                    pat = format!(".*?{pat}");
                }
            }

            if kind.is_glob() || kind.is_path() || kind.is_regex() {
                result.push(Pattern::new(kind, pat).with_exact_file(exact_file));
            } else if matches!(kind, PatternKind::ListFile | PatternKind::ListFile0) {
                let stream: Box<dyn std::io::Read> = if pat == "-" {
                    let Some(stdin_unwrapped) = stdin else {
                        return Err(Error::StdinUnavailable.into());
                    };

                    let result = Box::new(&mut *stdin_unwrapped);
                    // Ban subsequent usage of `listfile:-`
                    stdin = None;

                    result
                } else {
                    Box::new(fs_err::File::open(&pat)?)
                };
                let contents = std::io::read_to_string(stream)?;

                let (patterns, listfile_warnings) = if kind == PatternKind::ListFile {
                    normalize_patterns(
                        contents.lines(),
                        default_kind,
                        root,
                        cwd,
                        false,
                        // listfile:- is only allowed at the top-level:
                        None::<&mut R>,
                    )?
                } else {
                    normalize_patterns(
                        contents.split('\0'),
                        default_kind,
                        root,
                        cwd,
                        false,
                        // listfile:- is only allowed at the top-level:
                        None::<&mut R>,
                    )?
                };

                warnings.extend(listfile_warnings);

                if patterns.is_empty() {
                    warnings.push(format!("empty {} {pat} matches nothing", kind.name()));
                }

                for p in patterns {
                    result.push(p.with_source(pat.clone()));
                }
            } else {
                return Err(Error::UnsupportedPatternKind(kind.name().to_string()).into());
            }
        }
    }

    Ok((result, warnings))
}

// Extract "exact" file path from pattern. This is only appropriate to call at a
// particular moment during pattern normalization (see callsite).
fn exact_file(kind: PatternKind, mut pat: &str) -> Result<Option<RepoPathBuf>> {
    if (kind.is_path() || kind.is_glob())
        // rootfilesin only specifies a directory, not a file
        && kind != PatternKind::RootFilesIn
        // exclude free patterns (relglob)
        && !kind.is_free()
    {
        if kind.is_glob() {
            // Trim to longest path prefix without glob operator.
            if let Some(op_idx) = first_glob_operator_index(pat) {
                let slash_idx = pat[..op_idx].rfind('/').unwrap_or(0);
                pat = &pat[..slash_idx];
            }
        }

        // you can't have a file with no name
        if !pat.is_empty() {
            return Ok(Some(RepoPathBuf::from_string(pat.to_string())?));
        }
    }

    Ok(None)
}

/// A wrapper of `util::path::normalize` function by adding path separator conversion,
/// yields normalized [String] if the pattern is valid unicode.
///
/// This function normalize the path difference on Windows by converting
/// path separator from `\` to `/`. This is need because our `RepoPathBuf`
/// is a path separated by `/`.
fn normalize_path_pattern(pattern: &str) -> String {
    let pattern = util::path::normalize(pattern.as_ref());
    // SAFTEY: In Rust, values of type String are always valid UTF-8.
    // Our input pattern is a &str, and we don't add invalid chars in
    // out `util::path::normalize` function, so it should be safe here.
    let pattern = pattern.into_os_string().into_string().unwrap();
    if cfg!(windows) {
        pattern.replace(
            std::path::MAIN_SEPARATOR,
            &types::path::SEPARATOR.to_string(),
        )
    } else {
        pattern
    }
}

#[cfg(test)]
mod tests {

    use fs_err as fs;
    use tempfile::TempDir;
    use PatternKind::*;

    use super::*;

    #[test]
    fn test_split_pattern() {
        let v = split_pattern("re:a.*py", Glob);
        assert_eq!(v, (RE, "a.*py"));

        let v = split_pattern("badkind:a.*py", Glob);
        assert_eq!(v, (Glob, "badkind:a.*py"));

        let v = split_pattern("a.*py", RE);
        assert_eq!(v, (RE, "a.*py"));
    }

    #[test]
    fn test_pattern_kind_enum() {
        assert_eq!(PatternKind::from_str("re").unwrap(), RE);
        assert!(PatternKind::from_str("invalid").is_err());

        assert_eq!(RE.name(), "re");
    }

    #[test]
    fn test_normalize_path_pattern() {
        assert_eq!(
            normalize_path_pattern("foo/bar/../baz/"),
            "foo/baz".to_string()
        );
    }

    #[track_caller]
    fn normalize<R>(
        pat: &str,
        root: &str,
        cwd: &str,
        recursive: bool,
        stdin: &mut R,
    ) -> Result<(Vec<Pattern>, Vec<String>)>
    where
        R: std::io::Read,
    {
        // Caller must specify kind.
        assert!(pat.contains(':'));

        normalize_patterns(
            vec![pat],
            Glob,
            root.as_ref(),
            cwd.as_ref(),
            recursive,
            Some(stdin),
        )
    }

    #[track_caller]
    fn assert_normalize<R>(
        pat: &str,
        expected: &[&str],
        root: &str,
        cwd: &str,
        recursive: bool,
        stdin: &mut R,
    ) where
        R: std::io::Read,
    {
        let kind = pat.split_once(':').unwrap().0;
        let got: Vec<String> = normalize(pat, root, cwd, recursive, stdin)
            .unwrap()
            .0
            .into_iter()
            .map(|p| {
                assert_eq!(p.kind.name(), kind);
                p.pattern
            })
            .collect();

        assert_eq!(got, expected);
    }

    #[test]
    fn test_normalize_patterns() {
        #[track_caller]
        fn check(pat: &str, expected: &[&str]) {
            assert_normalize(
                pat,
                expected,
                "/root",
                "/root/cwd",
                false,
                &mut std::io::empty(),
            );
        }

        check("glob:", &["cwd"]);
        check("glob:.", &["cwd"]);
        check("glob:..", &[""]);
        check("glob:a", &["cwd/a"]);
        check("glob:../a{b,c}d", &["abd", "acd"]);
        check("glob:/root/foo/*.c", &["foo/*.c"]);

        check("relglob:", &[""]);
        check("relglob:.", &[""]);
        check("relglob:*.c", &["**/*.c"]);

        check("path:", &["**"]);
        check("path:.", &["**"]);
        check("path:foo", &["foo/**"]);
        check("path:foo*", &[r"foo\*/**"]);

        check("relpath:", &["cwd/**"]);
        check("relpath:.", &["cwd/**"]);
        check("relpath:foo", &["cwd/foo/**"]);
        check("relpath:../foo*", &[r"foo\*/**"]);

        check(r"re:a.*\.py", &[r"a.*\.py"]);

        check(r"relre:a.*\.py", &[r".*?a.*\.py"]);
        check(r"relre:^foo(bar|baz)", &[r"foo(bar|baz)"]);

        check("rootfilesin:", &["*"]);
        check("rootfilesin:.", &["*"]);
        check("rootfilesin:foo*", &[r"foo\*/*"]);
    }

    #[test]
    fn test_normalize_stdin() {
        #[track_caller]
        fn check(pat: &str, expected: &[&str]) {
            let got: Vec<String> = normalize(
                "listfile:-",
                "/root",
                "/root/cwd",
                false,
                &mut pat.as_bytes(),
            )
            .unwrap()
            .0
            .into_iter()
            .map(|p| p.pattern)
            .collect();

            assert_eq!(got, expected);
        }

        check("glob:", &["cwd"]);
        check("glob:.", &["cwd"]);
        check("glob:..", &[""]);
        check("glob:a", &["cwd/a"]);
        check("glob:../a{b,c}d", &["abd", "acd"]);
        check("glob:/root/foo/*.c", &["foo/*.c"]);

        check("relglob:", &[""]);
        check("relglob:.", &[""]);
        check("relglob:*.c", &["**/*.c"]);

        check("path:", &["**"]);
        check("path:.", &["**"]);
        check("path:foo", &["foo/**"]);
        check("path:foo*", &[r"foo\*/**"]);

        check("relpath:", &["cwd/**"]);
        check("relpath:.", &["cwd/**"]);
        check("relpath:foo", &["cwd/foo/**"]);
        check("relpath:../foo*", &[r"foo\*/**"]);

        check(r"re:a.*\.py", &[r"a.*\.py"]);

        check(r"relre:a.*\.py", &[r".*?a.*\.py"]);
        check(r"relre:^foo(bar|baz)", &[r"foo(bar|baz)"]);

        check("rootfilesin:", &["*"]);
        check("rootfilesin:.", &["*"]);
        check("rootfilesin:foo*", &[r"foo\*/*"]);
    }

    #[test]
    fn test_recursive_stdin() {
        let got: anyhow::Error = normalize_patterns(
            vec!["listfile:-"],
            Glob,
            "/root".as_ref(),
            "/root/cwd".as_ref(),
            false,
            Some(&mut "listfile:-".as_bytes()),
        )
        .unwrap_err();

        assert_eq!(
            got.to_string(),
            anyhow::Error::from(Error::StdinUnavailable).to_string()
        );
    }

    #[test]
    fn test_duplicate_stdin() {
        let got: anyhow::Error = normalize_patterns(
            vec!["listfile:-", "listfile:-"],
            Glob,
            "/root".as_ref(),
            "/root/cwd".as_ref(),
            false,
            Some(&mut std::io::empty()),
        )
        .unwrap_err();

        assert_eq!(
            got.to_string(),
            anyhow::Error::from(Error::StdinUnavailable).to_string()
        );
    }

    #[test]
    fn test_normalize_multiple() {
        let got: Vec<(PatternKind, String)> = normalize_patterns(
            vec!["naked", "relpath:foo/b{a}r", "glob:a{b,c}"],
            PatternKind::RelGlob,
            "/root".as_ref(),
            "/root/cwd".as_ref(),
            false,
            Some(&mut std::io::empty()),
        )
        .unwrap()
        .0
        .into_iter()
        .map(|p| (p.kind, p.pattern))
        .collect();

        assert_eq!(
            got,
            vec![
                (PatternKind::RelGlob, "**/naked".to_string()),
                (PatternKind::RelPath, "cwd/foo/b\\{a\\}r/**".to_string()),
                (PatternKind::Glob, "cwd/ab".to_string()),
                (PatternKind::Glob, "cwd/ac".to_string()),
            ]
        );
    }

    #[test]
    fn test_recursive_normalize() {
        #[track_caller]
        fn check(pat: &str, expected: &[&str]) {
            assert_normalize(
                pat,
                expected,
                "/root",
                "/root/cwd",
                true,
                &mut std::io::empty(),
            );
        }

        check("glob:", &["cwd/**"]);
        check("glob:/root", &["**"]);
    }

    #[test]
    fn test_normalize_patterns_unsupported_kind() {
        assert!(
            normalize_patterns(
                vec!["set:added()"],
                Glob,
                "/".as_ref(),
                "/".as_ref(),
                false,
                Some(&mut std::io::empty()),
            )
            .is_err()
        );
    }

    #[test]
    fn test_build_patterns() {
        let patterns = ["re:a.py".to_string(), "a.txt".to_string()];

        assert_eq!(
            build_patterns(&patterns, Glob),
            [
                Pattern::new(RE, "a.py".to_string()),
                Pattern::new(Glob, "a.txt".to_string())
            ]
        )
    }

    #[test]
    fn test_normalize_patterns_listfile() {
        test_normalize_patterns_listfile_helper("\n");
        test_normalize_patterns_listfile_helper("\r\n");
    }

    #[test]
    fn test_normalize_patterns_listfile0() {
        test_normalize_patterns_listfile_helper("\0");
    }

    fn test_normalize_patterns_listfile_helper(sep: &str) {
        let inner_patterns = ["glob:/a/*", r"re:a.*\.py"];
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("patterns.txt");
        let path_str = path.to_string_lossy();
        let content = inner_patterns.join(sep);
        fs::write(&path, content).unwrap();

        let outer_patterns = vec![format!(
            "listfile{}:{}",
            if sep == "\0" { "0" } else { "" },
            path_str
        )];
        let result = normalize_patterns(
            outer_patterns,
            Glob,
            "/".as_ref(),
            "/".as_ref(),
            false,
            Some(&mut std::io::empty()),
        )
        .unwrap()
        .0;

        assert_eq!(
            result,
            [
                Pattern::new(Glob, "a/*".to_string())
                    .with_source(path_str.to_string())
                    .with_exact_file(Some("a".to_string().try_into().unwrap())),
                Pattern::new(RE, r"a.*\.py".to_string()).with_source(path_str.to_string())
            ]
        )
    }

    #[test]
    fn test_exact_file() {
        #[track_caller]
        fn check(pat: &str, expected: &[&str]) {
            let got: Vec<String> =
                normalize(pat, "/root", "/root/cwd", false, &mut std::io::empty())
                    .unwrap()
                    .0
                    .into_iter()
                    .filter_map(|p| p.exact_file.map(|p| p.to_string()))
                    .collect();

            assert_eq!(got, expected);
        }

        check("path:", &[]);
        check("path:.", &[]);
        check("path:foo", &["foo"]);
        check("path:foo*/bar?", &["foo*/bar?"]);

        check("relpath:foo", &["cwd/foo"]);
        check("relpath:", &["cwd"]);

        check("glob:foo", &["cwd/foo"]);
        check("glob:foo*", &["cwd"]);
        check("glob:foo/*/baz", &["cwd/foo"]);
        check("glob:/root/foo*/*/baz", &[]);
        check("glob:/root/*foo", &[]);
        check("glob:/root/foo/bar*/baz", &["foo"]);
        check("glob:/root/foo/bar/baz?", &["foo/bar"]);

        check("relglob:foo", &[]);

        check("rootfilesin:foo", &[]);
    }
}
