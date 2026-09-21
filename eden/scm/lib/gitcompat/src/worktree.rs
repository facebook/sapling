/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io;
use std::path::PathBuf;

use encoding::shell_output_bytes_to_path;

use crate::GitCmd;
use crate::RepoGit;

#[derive(Debug, Eq, PartialEq)]
/// A working tree reported by `git worktree list`.
pub struct GitWorktree {
    /// Absolute path to the working tree.
    pub path: PathBuf,
    /// Whether this is the repository's main working tree.
    pub is_main: bool,
}

impl RepoGit {
    /// List the worktrees registered with Git.
    pub fn list_worktrees(&self) -> io::Result<Vec<GitWorktree>> {
        let output = self.call("worktree", &["list", "--porcelain", "-z"])?;
        parse_worktrees(&output.stdout)
    }
}

fn parse_worktrees(output: &[u8]) -> io::Result<Vec<GitWorktree>> {
    // Git guarantees that the main worktree is listed first, before the
    // path-sorted linked worktrees.
    output
        .split(|byte| *byte == 0)
        .filter_map(|field| field.strip_prefix(b"worktree "))
        .enumerate()
        .map(|(index, path)| {
            Ok(GitWorktree {
                path: shell_output_bytes_to_path(path)?.into_owned(),
                is_main: index == 0,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_worktrees() {
        let output = b"worktree /repo/main\0HEAD 1111111111111111111111111111111111111111\0branch refs/heads/main\0\0worktree /repo/linked\0HEAD 2222222222222222222222222222222222222222\0detached\0locked reason with spaces\0\0";

        assert_eq!(
            parse_worktrees(output).expect("valid porcelain output"),
            vec![
                GitWorktree {
                    path: PathBuf::from("/repo/main"),
                    is_main: true,
                },
                GitWorktree {
                    path: PathBuf::from("/repo/linked"),
                    is_main: false,
                },
            ]
        );
    }

    #[test]
    fn test_parse_empty_worktree_list() {
        assert!(
            parse_worktrees(b"")
                .expect("empty output is valid")
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_parse_non_utf8_worktree_path() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let worktrees = parse_worktrees(b"worktree /repo/non-utf8-\xff\0HEAD 1111\0\0")
            .expect("non-UTF-8 paths are valid on Unix");

        assert_eq!(
            worktrees[0].path,
            PathBuf::from(OsStr::from_bytes(b"/repo/non-utf8-\xff"))
        );
    }
}
