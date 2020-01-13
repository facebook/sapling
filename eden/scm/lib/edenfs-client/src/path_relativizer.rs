/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

// TODO: Consider cleaning up and moving this to utils::path.
fn relativize(base: &PathBuf, path: &PathBuf) -> PathBuf {
    let mut base_iter = base.iter();
    let mut path_iter = path.iter();
    let mut rel_path = PathBuf::new();
    loop {
        match (base_iter.next(), path_iter.next()) {
            (Some(ref c), Some(ref p)) if c == p => continue,

            // Examples:
            // b: foo/bar/baz/
            // p: foo/bar/biz/buzz.html
            // This is the common case where we have to go up some number of directories
            // (so one "../" per unique path component of base) and then back down.
            //
            // b: foo/bar/baz/biz/
            // p: foo/bar/
            // If foo/bar was a file and then the user replaced it with a directory, and now
            // the user is in a subdirectory of that directory, then one "../" per unique path
            // component of base.
            (Some(_c), remaining_path) => {
                // Find the common prefix of path and base. Prefix with one "../" per unique
                // path component of base and then append the unique sequence of components from
                // path.
                rel_path.push(".."); // This is for the current component, c.
                for _ in base_iter {
                    rel_path.push("..");
                }

                if let Some(p) = remaining_path {
                    rel_path.push(p);
                    for component in path_iter {
                        rel_path.push(component);
                    }
                }
                break;
            }

            // Example:
            // b: foo/bar/
            // p: foo/bar/baz/buzz.html
            (None, Some(p)) => {
                rel_path.push(p);
                for component in path_iter {
                    rel_path.push(component);
                }
                break;
            }

            // Example:
            // b: foo/bar/baz/
            // p: foo/bar/baz/
            // If foo/bar/baz was a file and then the user replaced it with a directory, which
            // is also the user's current directory, then "" should be returned.
            (None, None) => {
                break;
            }
        }
    }

    rel_path
}

enum PathRelativizerConfig {
    // If the cwd is inside the repo, then Hg paths should be relativized against the cwd relative
    // to the repo root.
    CwdUnderRepo { relative_cwd: PathBuf },

    // If the cwd is outside the repo, then prefix is the cwd relative to the repo root: Hg paths
    // can simply be appended to this path.
    CwdOutsideRepo { prefix: PathBuf },
}

pub struct PathRelativizer {
    config: PathRelativizerConfig,
}

/// Utility for computing a relativized path for a file in an Hg repository given the user's cwd
/// and specified value for --repository/-R, if any.
impl PathRelativizer {
    /// `cwd` corresponds to getcwd(2) while `repo_root` is the absolute path specified via
    /// --repository/-R, or failing that, the Hg repo that contains `cwd`.
    pub fn new(cwd: PathBuf, repo_root: PathBuf) -> PathRelativizer {
        use self::PathRelativizerConfig::*;
        let config = if cwd.starts_with(&repo_root) {
            CwdUnderRepo {
                relative_cwd: relativize(&repo_root, &cwd),
            }
        } else {
            CwdOutsideRepo {
                prefix: relativize(&cwd, &repo_root),
            }
        };
        PathRelativizer { config }
    }

    /// `path` must be relative to the repo root. Returns a corresponding PathBuf that is suitable
    /// via display on the console.
    pub fn relativize(&self, path: &PathBuf) -> PathBuf {
        use self::PathRelativizerConfig::*;
        match self.config {
            CwdUnderRepo { ref relative_cwd } => relativize(relative_cwd, path),
            CwdOutsideRepo { ref prefix } => prefix.join(path),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn relativize_absolute_paths() {
        let check = |base, path, expected| {
            let base_buf = PathBuf::from(base);
            let path_buf = PathBuf::from(path);
            assert_eq!(relativize(&base_buf, &path_buf), PathBuf::from(expected));
        };
        check("/", "/", "");
        check("/foo/bar/baz", "/foo/bar/baz", "");
        check("/foo/bar", "/foo/bar/baz", "baz");
        check("/foo", "/foo/bar/baz", "bar/baz");
        check("/foo/bar/baz", "/foo/bar", "..");
        check("/foo/bar/baz", "/foo", "../..");
        check("/foo/bar/baz", "/foo/BAR", "../../BAR");
        check("/foo/bar/baz", "/foo/BAR/BAZ", "../../BAR/BAZ");
    }

    #[test]
    fn relativize_path_from_repo_when_cwd_is_repo_root() {
        let repo_root = PathBuf::from("/home/zuck/tfb");
        let cwd = PathBuf::from("/home/zuck/tfb");
        let relativizer = PathRelativizer::new(cwd, repo_root);
        let check = |path, expected| {
            let path_buf = PathBuf::from(path);
            assert_eq!(relativizer.relativize(&path_buf), PathBuf::from(expected));
        };
        check("foo/bar.txt", "foo/bar.txt");
    }

    #[test]
    fn relativize_path_from_repo_when_cwd_is_descendant_of_repo_root() {
        let repo_root = PathBuf::from("/home/zuck/tfb");
        let cwd = PathBuf::from("/home/zuck/tfb/foo");
        let relativizer = PathRelativizer::new(cwd, repo_root);
        let check = |path, expected| {
            let path_buf = PathBuf::from(path);
            assert_eq!(relativizer.relativize(&path_buf), PathBuf::from(expected));
        };
        check("foo/bar.txt", "bar.txt");
    }

    #[test]
    fn relativize_path_from_repo_when_cwd_is_ancestor_of_repo_root() {
        let repo_root = PathBuf::from("/home/zuck/tfb");
        let cwd = PathBuf::from("/home/zuck");
        let relativizer = PathRelativizer::new(cwd, repo_root);
        let check = |path, expected| {
            let path_buf = PathBuf::from(path);
            assert_eq!(relativizer.relativize(&path_buf), PathBuf::from(expected));
        };
        check("foo/bar.txt", "tfb/foo/bar.txt");
    }

    #[test]
    fn relativize_path_from_repo_when_cwd_is_cousin_of_repo_root() {
        let repo_root = PathBuf::from("/home/zuck/tfb");
        let cwd = PathBuf::from("/home/schrep/tfb");
        let relativizer = PathRelativizer::new(cwd, repo_root);
        let check = |path, expected| {
            let path_buf = PathBuf::from(path);
            assert_eq!(relativizer.relativize(&path_buf), PathBuf::from(expected));
        };
        check("foo/bar.txt", "../../zuck/tfb/foo/bar.txt");
    }
}
