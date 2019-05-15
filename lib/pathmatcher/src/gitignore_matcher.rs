// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use ignore::{self, gitignore};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// Lazy `.gitignore` matcher that loads `.gitignore` files on demand.
pub struct GitignoreMatcher {
    ignore: gitignore::Gitignore,

    // PERF: Each Gitignore object stores "root" as "PathBuf" to support
    // matching against an absolute path. Since we enforce relative path
    // in the API, removing that "PathBuf" could reduce memory footprint.
    submatchers: RefCell<HashMap<PathBuf, Box<GitignoreMatcher>>>,

    cached: MatchResult,
}

/// Return (next_component, remaining_path), or None if remaining_path is empty.
fn split_path(path: &Path) -> Option<(&Path, &Path)> {
    let mut comps = path.components();
    let comp = comps.next();
    comp.and_then(|c| {
        let rest = comps.as_path();
        if let Component::Normal(s) = c {
            if rest.as_os_str().is_empty() {
                None
            } else {
                Some((Path::new(s), rest))
            }
        } else {
            panic!("ProgrammingError: unexpected path component {:?}", &c);
        }
    })
}

#[derive(Debug, PartialEq, Clone)]
pub enum MatchResult {
    Unspecified,
    Ignored { file: String, glob: String },
    Whitelisted { file: String, glob: String },
}

fn nice_path(path: Option<&Path>, root_path: &Path) -> String {
    match path {
        Some(v) => v
            .strip_prefix(root_path)
            .unwrap_or(v)
            .to_string_lossy()
            .to_string(),
        None => "".to_string(),
    }
}

impl MatchResult {
    pub fn bool_ignore(&self) -> bool {
        match self {
            MatchResult::Ignored { .. } => true,
            _ => false,
        }
    }

    pub fn explain(&self) -> String {
        match self {
            MatchResult::Ignored { file, glob } => format!("Ignored by rule {} in {}", glob, file),
            MatchResult::Whitelisted { file, glob } => {
                format!("Whitelisted by rule {} in {}", glob, file)
            }
            MatchResult::Unspecified => "Unspecified".to_string(),
        }
    }

    fn from_glob(v: ignore::Match<&ignore::gitignore::Glob>, root_path: &Path) -> Self {
        match v {
            ignore::Match::None => MatchResult::Unspecified,
            ignore::Match::Ignore(blob) => MatchResult::Ignored {
                file: nice_path(blob.from(), root_path),
                glob: blob.original().to_string(),
            },
            ignore::Match::Whitelist(blob) => MatchResult::Whitelisted {
                file: nice_path(blob.from(), root_path),
                glob: blob.original().to_string(),
            },
        }
    }
}

impl GitignoreMatcher {
    /// Initialize `GitignoreMatch` for the given root directory.
    ///
    /// The `.gitignore` in the root directory will be parsed immediately.
    /// `.gitignore` in subdirectories are parsed lazily.
    ///
    /// `global_gitignore_paths` is an additional list of gitignore files
    /// to be parsed.
    pub fn new<P: AsRef<Path>>(root: P, global_gitignore_paths: Vec<&Path>) -> Self {
        let root = root.as_ref();
        let mut builder = gitignore::GitignoreBuilder::new(root);
        for path in global_gitignore_paths {
            builder.add(path);
        }
        builder.add(root.join(".gitignore"));
        let ignore = builder
            .build()
            .unwrap_or_else(|_| gitignore::Gitignore::empty());

        let submatchers = RefCell::new(HashMap::new());
        GitignoreMatcher {
            ignore,
            submatchers,
            cached: MatchResult::Unspecified,
        }
    }

    /// Like `new`, but might mark the subtree with a cached response
    /// Used internally by `match_subdir_path`.
    fn new_with_rootmatcher(dir: &Path, root: &GitignoreMatcher) -> Self {
        let dir_root_relative = dir.strip_prefix(root.ignore.path()).unwrap();
        let submatchers = RefCell::new(HashMap::new());
        // Cache only "ignored" results
        let cached = match root.match_relative(dir_root_relative, true) {
            MatchResult::Ignored { file, glob } => MatchResult::Ignored { file, glob },
            _ => MatchResult::Unspecified,
        };
        let ignore = match cached {
            MatchResult::Unspecified => gitignore::Gitignore::new(dir.join(".gitignore")).0,
            _ => gitignore::Gitignore::empty(),
        };
        GitignoreMatcher {
            ignore,
            cached,
            submatchers,
        }
    }

    /// Return true if the normalized relative path should be ignored.
    ///
    /// Panic if the path is not relative, or contains components like
    /// ".." or ".".
    pub fn match_relative<P: AsRef<Path>>(&self, path: P, is_dir: bool) -> MatchResult {
        let path = path.as_ref();
        self.match_path(path, is_dir, self)
    }

    /// Check .gitignore for the relative path.
    fn match_path(&self, path: &Path, is_dir: bool, root: &GitignoreMatcher) -> MatchResult {
        // Everything is ignored regardless if this directory is ignored.
        match self.cached {
            MatchResult::Unspecified => {
                // Check subdir first. It can override this (parent) directory.
                let subdir_result = match split_path(path) {
                    None => MatchResult::Unspecified,
                    Some((dir, rest)) => self.match_subdir_path(dir, rest, is_dir, root),
                };

                match subdir_result {
                    MatchResult::Unspecified => MatchResult::from_glob(
                        self.ignore.matched(path, is_dir),
                        root.ignore.path(),
                    ),
                    v => v,
                }
            }
            // If we have a cached result, return it
            ref v => v.clone(),
        }
    }

    /// Check .gitignore in the subdirectory `name` for the path `rest`.
    /// Create submatcher on demand.
    fn match_subdir_path(
        &self,
        name: &Path,
        rest: &Path,
        is_dir: bool,
        root: &GitignoreMatcher,
    ) -> MatchResult {
        {
            let submatchers = self.submatchers.borrow();
            if let Some(m) = submatchers.get(name) {
                return m.as_ref().match_path(rest, is_dir, root);
            }
        }
        {
            let dir = self.ignore.path().join(name);
            if dir.is_dir() {
                let m = GitignoreMatcher::new_with_rootmatcher(&dir, root);
                let result = m.match_path(rest, is_dir, root);
                let mut submatchers = self.submatchers.borrow_mut();
                submatchers.insert(name.to_path_buf(), Box::new(m));
                result
            } else {
                MatchResult::Unspecified
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, File};
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_split_path() {
        let p = Path::new("proc/self/stat");

        let (c, p) = split_path(p).unwrap();
        assert_eq!(c, Path::new("proc"));
        assert_eq!(p, Path::new("self/stat"));

        let (c, p) = split_path(p).unwrap();
        assert_eq!(c, Path::new("self"));
        assert_eq!(p, Path::new("stat"));

        assert!(split_path(p).is_none());
    }

    #[test]
    fn test_gitignore_match_directory() {
        let dir = tempdir().unwrap();
        let filename = dir.path().join(".gitignore");
        write(&filename, b"FILE\nDIR/\n");

        let m = GitignoreMatcher::new(dir.path(), Vec::new());
        assert!(m.match_relative("x/FILE", false).bool_ignore());
        assert_eq!(
            m.match_relative("x/FILE", false),
            MatchResult::Ignored {
                file: ".gitignore".to_string(),
                glob: "FILE".to_string(),
            }
        );
        assert!(m.match_relative("x/FILE", true).bool_ignore());
        assert_eq!(
            m.match_relative("x/FILE", true),
            MatchResult::Ignored {
                file: ".gitignore".to_string(),
                glob: "FILE".to_string(),
            }
        );
        assert!(!m.match_relative("x/DIR", false).bool_ignore());
        assert_eq!(m.match_relative("x/DIR", false), MatchResult::Unspecified);
        assert!(m.match_relative("x/DIR", true).bool_ignore());
        assert_eq!(
            m.match_relative("x/DIR", true),
            MatchResult::Ignored {
                file: ".gitignore".to_string(),
                glob: "DIR/".to_string(),
            }
        );
    }

    #[test]
    fn test_gitignore_match_subdir() {
        let dir = tempdir().unwrap();

        create_dir_all(dir.path().join("a/b")).expect("mkdir");
        create_dir_all(dir.path().join("c/d")).expect("mkdir");
        write(dir.path().join(".gitignore"), b"a/b\n!c/d/*");
        write(dir.path().join("a/b/.gitignore"), b"!c");
        write(dir.path().join("a/.gitignore"), b"!b/d");
        write(dir.path().join("c/.gitignore"), b"d/e\n!d/f");
        write(dir.path().join("c/d/.gitignore"), b"!e\nf");

        let m = GitignoreMatcher::new(dir.path(), Vec::new());
        assert!(m.match_relative("a/b", false).bool_ignore());
        assert_eq!(
            m.match_relative("a/b", false),
            MatchResult::Ignored {
                file: ".gitignore".to_string(),
                glob: "a/b".to_string(),
            }
        );
        assert!(m.match_relative("a/b/c", false).bool_ignore());
        assert_eq!(
            m.match_relative("a/b/c", false),
            MatchResult::Ignored {
                file: ".gitignore".to_string(),
                glob: "a/b".to_string(),
            }
        );
        assert!(m.match_relative("a/b/d", false).bool_ignore());
        assert_eq!(
            m.match_relative("a/b/d", false),
            MatchResult::Ignored {
                file: ".gitignore".to_string(),
                glob: "a/b".to_string(),
            }
        );
        #[cfg(unix)]
        {
            assert!(m.match_relative("c/d/f", false).bool_ignore());
            assert_eq!(
                m.match_relative("c/d/f", false),
                MatchResult::Ignored {
                    file: "c/d/.gitignore".to_string(),
                    glob: "f".to_string(),
                }
            );
            assert!(!m.match_relative("c/d/e", false).bool_ignore());
            assert_eq!(
                m.match_relative("c/d/e", false),
                MatchResult::Whitelisted {
                    file: "c/d/.gitignore".to_string(),
                    glob: "!e".to_string(),
                }
            );
        }
    }

    #[test]
    fn test_global_gitignore() {
        let dir = tempdir().unwrap();
        let ignore1_path = dir.path().join("ignore1");
        let ignore2_path = dir.path().join("ignore2");

        write(&ignore1_path, b"a*");
        write(&ignore2_path, b"b*");

        let m = GitignoreMatcher::new(dir.path(), vec![&ignore1_path, &ignore2_path]);
        assert!(m.match_relative("a1", true).bool_ignore());
        assert_eq!(
            m.match_relative("a1", true),
            MatchResult::Ignored {
                file: "ignore1".to_string(),
                glob: "a*".to_string(),
            }
        );
        assert!(m.match_relative("b1", true).bool_ignore());
        assert_eq!(
            m.match_relative("b1", true),
            MatchResult::Ignored {
                file: "ignore2".to_string(),
                glob: "b*".to_string(),
            }
        );
    }

    fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) {
        File::create(path)
            .expect("create")
            .write_all(contents.as_ref())
            .expect("write");
    }
}
