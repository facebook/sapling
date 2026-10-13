/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

/// Usually `.git` is a directory. However, a submodule or linked worktree created by `git`
/// uses a file `.git` with the content `gitdir: ...`. This function attempts
/// to follow the file to get a directory. Best-effort.
///
/// In Git terminology, returns `$GIT_DIR`, not `$GIT_COMMON_DIR`.
pub fn follow_dotgit_path(mut git_dir: PathBuf) -> PathBuf {
    for _patience in 0..4 {
        let metadata = match fs::metadata(&git_dir) {
            Err(_) => break,
            Ok(v) => v,
        };
        if metadata.is_file() {
            let content = match fs::read_to_string(&git_dir) {
                Err(_) => break,
                Ok(v) => v,
            };
            if let (Some(path), Some(root)) =
                (content.trim().strip_prefix("gitdir: "), git_dir.parent())
            {
                let new_git_dir = root.join(path);
                tracing::trace!(
                    "follow dotgit {} => {}",
                    git_dir.display(),
                    new_git_dir.display()
                );
                git_dir = new_git_dir;
                continue;
            }
        }
        break;
    }
    util::path::normalize(&git_dir)
}

/// Resolve Git's `GIT_COMMON_DIR` from an already resolved `GIT_DIR`.
///
/// A linked worktree records the path, relative to `GIT_DIR`, in its `commondir` file. For other
/// repositories, where that file does not exist, `GIT_COMMON_DIR` is the same as `GIT_DIR`.
pub fn resolve_git_common_dir(git_dir: &Path) -> io::Result<PathBuf> {
    match fs::read_to_string(git_dir.join("commondir")) {
        Ok(path) => Ok(git_dir.join(path.trim_end())),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(git_dir.to_path_buf()),
        Err(err) => Err(err),
    }
}

/// `.git` mode specific logic to resolve `dot_dir`.
///
/// Returns the per-worktree Sapling dot dir `$GIT_DIR/sl`, not `$GIT_COMMON_DIR/sl`.
pub fn resolve_dot_dir_func(root: &Path, dot_dir: &'static str) -> PathBuf {
    assert!(dot_dir.starts_with(".git"));
    let dot_git_path = &follow_dotgit_path(root.join(".git"));
    dot_git_path.join("sl")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_git_common_dir() -> io::Result<()> {
        let temp_dir = tempfile::tempdir()?;
        let git_dir = temp_dir.path().join("common/worktrees/topic");
        fs::create_dir_all(&git_dir)?;

        assert_eq!(resolve_git_common_dir(&git_dir)?, git_dir);

        fs::write(git_dir.join("commondir"), "../..")?;
        assert_eq!(resolve_git_common_dir(&git_dir)?, git_dir.join("../.."));

        Ok(())
    }
}
