/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::path::Path;
use std::path::PathBuf;

/// Usually `.git` is a directory. However, submodule repo created by `git`
/// uses a file `.git` with the content `gitdir: ...`. This function attempts
/// to follow the file to get a directory. Best-effort.
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

/// Resolve the "common dir" for a git directory.
///
/// In a git worktree, `git_dir` points to `.git/worktrees/<name>/` which is
/// worktree-specific. Shared resources (refs/heads, packed-refs, objects, etc.)
/// live in the main `.git/` directory. The `commondir` file in the worktree dir
/// tells git where to find them.
///
/// For a regular (non-worktree) repo, there is no `commondir` file, so this
/// returns `git_dir` unchanged.
pub fn resolve_common_dir(git_dir: &Path) -> PathBuf {
    let commondir_path = git_dir.join("commondir");
    match fs::read_to_string(&commondir_path) {
        Ok(content) => {
            let relative = content.trim();
            if relative.is_empty() {
                return git_dir.to_path_buf();
            }
            let common = if Path::new(relative).is_absolute() {
                PathBuf::from(relative)
            } else {
                git_dir.join(relative)
            };
            util::path::normalize(&common)
        }
        Err(_) => git_dir.to_path_buf(),
    }
}

/// `.git` mode specific logic to resolve `dot_dir`.
pub fn resolve_dot_dir_func(root: &Path, dot_dir: &'static str) -> PathBuf {
    assert!(dot_dir.starts_with(".git"));
    let dot_git_path = &follow_dotgit_path(root.join(".git"));
    dot_git_path.join("sl")
}
