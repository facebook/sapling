/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use crate::error::Error;

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

    /// a file of patterns to read and include
    Include,

    /// a file of patterns to match against files under the same directory
    SubInclude,

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
            PatternKind::Include => "include",
            PatternKind::SubInclude => "subinclude",
            PatternKind::RootFilesIn => "rootfilesin",
        }
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
            "include" => Ok(PatternKind::Include),
            "subinclude" => Ok(PatternKind::SubInclude),
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
}

impl Pattern {
    pub(crate) fn new(kind: PatternKind, pattern: String) -> Self {
        Self {
            kind,
            pattern,
            source: None,
        }
    }

    pub(crate) fn with_source(mut self, source: String) -> Self {
        self.source = Some(source);
        self
    }
}

pub fn split_pattern<'a>(pattern: &'a str, default_kind: PatternKind) -> (PatternKind, &'a str) {
    match pattern.split_once(':') {
        Some((k, p)) => {
            if let Ok(kind) = PatternKind::from_str(k) {
                (kind, p)
            } else {
                (default_kind, pattern)
            }
        }
        None => (default_kind, pattern),
    }
}

// TODO: refactor this code to avoid the overhead of monomorphization by
// using a wrapper function.
#[allow(dead_code)]
pub(crate) fn normalize_patterns<I>(
    patterns: I,
    default_kind: PatternKind,
) -> Result<Vec<Pattern>, Error>
where
    I: IntoIterator,
    I::Item: AsRef<str>,
{
    let mut result: Vec<Pattern> = Vec::new();
    for pattern in patterns {
        let pattern = pattern.as_ref();
        let (kind, pat) = split_pattern(pattern, default_kind);
        match kind {
            PatternKind::RelPath | PatternKind::Glob => {
                // TODO: need to implement pathutil.pathauditor and pathutil.canonpath
                // https://fburl.com/code/0q9sgvbj
                result.push(Pattern::new(kind, pat.to_string()));
            }
            PatternKind::RelGlob | PatternKind::Path | PatternKind::RootFilesIn => {
                let normalized_pat = normalize_path_pattern(pat);
                result.push(Pattern::new(kind, normalized_pat));
            }
            PatternKind::ListFile | PatternKind::ListFile0 => {
                let contents = util::file::read_to_string(pat)?;
                let sep = if kind == PatternKind::ListFile {
                    '\n'
                } else {
                    '\0'
                };
                let lines = contents.split(sep);
                for p in normalize_patterns(lines, default_kind)? {
                    let p = p.with_source(pat.to_string());
                    result.push(p);
                }
            }
            PatternKind::Set | PatternKind::Include | PatternKind::SubInclude => {
                return Err(Error::UnsupportedPatternKind(kind.name().to_string()));
            }
            _ => result.push(Pattern::new(kind, pat.to_string())),
        }
    }
    Ok(result)
}

/// A wrapper of `util::path::normalize` function by adding path separator convertion,
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
    let pattern_str = pattern.to_string_lossy();
    if cfg!(windows) {
        pattern_str.replace(
            std::path::MAIN_SEPARATOR,
            &types::path::SEPARATOR.to_string(),
        )
    } else {
        pattern_str.to_string()
    }
}

#[cfg(test)]
mod tests {

    use std::fs;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_split_pattern() {
        let v = split_pattern("re:a.*py", PatternKind::Glob);
        assert_eq!(v, (PatternKind::RE, "a.*py"));

        let v = split_pattern("badkind:a.*py", PatternKind::Glob);
        assert_eq!(v, (PatternKind::Glob, "badkind:a.*py"));

        let v = split_pattern("a.*py", PatternKind::RE);
        assert_eq!(v, (PatternKind::RE, "a.*py"));
    }

    #[test]
    fn test_pattern_kind_enum() {
        assert_eq!(PatternKind::from_str("re").unwrap(), PatternKind::RE);
        assert!(PatternKind::from_str("invalid").is_err());

        assert_eq!(PatternKind::RE.name(), "re");
    }

    #[test]
    fn test_normalize_path_pattern() {
        assert_eq!(
            normalize_path_pattern("foo/bar/../baz/"),
            "foo/baz".to_string()
        );
    }

    #[test]
    fn test_normalize_patterns() {
        assert_eq!(
            normalize_patterns(
                vec!["glob:/a/*", r"re:a.*\.py", "path:foo/bar/../baz/"],
                PatternKind::Glob
            )
            .unwrap(),
            [
                Pattern::new(PatternKind::Glob, "/a/*".to_string()),
                Pattern::new(PatternKind::RE, r"a.*\.py".to_string()),
                Pattern::new(PatternKind::Path, "foo/baz".to_string()),
            ]
        );
        assert_eq!(
            normalize_patterns(vec!["/a/*", r"re:a.*\.py"], PatternKind::Glob).unwrap(),
            [
                Pattern::new(PatternKind::Glob, "/a/*".to_string()),
                Pattern::new(PatternKind::RE, r"a.*\.py".to_string()),
            ]
        );
        assert_eq!(
            normalize_patterns(vec!["relglob:*.c"], PatternKind::Glob).unwrap(),
            [Pattern::new(PatternKind::RelGlob, "*.c".to_string()),]
        );
    }

    #[test]
    fn test_normalize_patterns_unsupported_kind() {
        assert!(normalize_patterns(vec!["set:added()"], PatternKind::Glob).is_err());
        assert!(normalize_patterns(vec!["include:/a/b.txt"], PatternKind::Glob).is_err());
        assert!(normalize_patterns(vec!["subinclude:/a/b.txt"], PatternKind::Glob).is_err());
    }

    #[test]
    fn test_normalize_patterns_listfile() {
        test_normalize_patterns_listfile_helper("\n");
    }

    #[test]
    fn test_normalize_patterns_listfile0() {
        test_normalize_patterns_listfile_helper("\0");
    }

    fn test_normalize_patterns_listfile_helper(sep: &str) {
        let inner_patterns = vec!["glob:/a/*", r"re:a.*\.py"];
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("patterns.txt");
        let path_str = path.to_string_lossy();
        let content = inner_patterns.join(sep);
        fs::write(&path, content).unwrap();

        let outer_patterns = vec![format!(
            "listfile{}:{}",
            if sep == "\n" { "" } else { "0" },
            path_str
        )];
        let result = normalize_patterns(outer_patterns, PatternKind::Glob).unwrap();

        assert_eq!(
            result,
            [
                Pattern::new(PatternKind::Glob, "/a/*".to_string())
                    .with_source(path_str.to_string()),
                Pattern::new(PatternKind::RE, r"a.*\.py".to_string())
                    .with_source(path_str.to_string())
            ]
        )
    }
}
