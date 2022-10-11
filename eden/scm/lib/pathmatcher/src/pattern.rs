/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use crate::error::Error;

#[derive(Debug, PartialEq)]
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

#[cfg(test)]
mod tests {
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
}
