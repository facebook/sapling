/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use hg_util::path::expand_path;

pub mod jsonrpc;

/// Expand the path if the user has supplied anything. Otherwise, use the current working directory instead.
///
/// Usage:
/// ```no_run
/// #[clap(..., parse(try_from_str = expand_path_or_cwd), default_value = "", ...)]
/// ```
pub fn expand_path_or_cwd(input: &str) -> Result<PathBuf> {
    // TODO(T135653638): This function must be updated when expand_path is updated to use OsString
    if input.is_empty() {
        std::env::current_dir().context("Unable to retrieve current working directory")
    } else {
        Ok(expand_path(input))
    }
}

/// Utility function to remove trailing slashes from user provided relative paths. This is required
/// because some EdenFS internals do not allow trailing slashses on relative paths.
pub fn remove_trailing_slash(path: &Path) -> PathBuf {
    // TODO(T135653638): This function must be updated when expand_path is updated to use OsString
    PathBuf::from(
        path.to_string_lossy()
            .trim_end_matches(if cfg!(windows) { r"\" } else { "/" }),
    )
}

/// Traverse up and locate the repository root
pub fn locate_repo_root(path: &Path) -> Option<&Path> {
    path.ancestors()
        .find(|p| p.join(".hg").is_dir() || p.join(".git").is_dir())
}

pub fn locate_eden_config_dir(path: &Path) -> Option<PathBuf> {
    // Check whether we're in an Eden mount. If we are, some parent directory will contain
    // a .eden dir that contains a socket file. This socket file is symlinked to the
    // socket file contained in the config dir we should use for this mount.
    if let Ok(expanded_path) = path.canonicalize() {
        for ancestor in expanded_path.ancestors() {
            let socket = ancestor.join(".eden").join("socket");
            if socket.exists() {
                if let Ok(resolved_socket) = socket.canonicalize() {
                    if let Some(parent) = resolved_socket.parent() {
                        return Some(parent.to_path_buf());
                    }
                }
            }
        }
    }
    None
}
