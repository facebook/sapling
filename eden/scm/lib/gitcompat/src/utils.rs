/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
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
    git_dir
}
