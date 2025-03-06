/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::env;
use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use hg_util::path::expand_path;

use crate::instance::DEFAULT_CONFIG_DIR;
use crate::instance::DEFAULT_ETC_EDEN_DIR;

/// Traverse up and locate the repository root
pub fn locate_repo_root(path: &Path) -> Option<&Path> {
    path.ancestors()
        .find(|p| p.join(".hg").is_dir() || p.join(".git").is_dir())
}

pub fn get_mount_point(mount_point: &Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = mount_point {
        Ok(path.clone())
    } else {
        locate_repo_root(
            &std::env::current_dir().context("Unable to retrieve current working directory")?,
        )
        .map(|p| p.to_path_buf())
        .ok_or_else(|| anyhow!("Unable to locate repository root"))
    }
}

/// Expand the path if the user has supplied anything. Otherwise, use the current working directory instead.
///
/// Usage:
/// ```ignore
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

pub fn get_config_dir(
    config_dir_override: &Option<PathBuf>,
    mount_path_override: &Option<PathBuf>,
) -> Result<PathBuf> {
    // A config dir might be provided as a top-level argument. Top-level arguments take
    // precedent over sub-command args.
    if let Some(config_dir) = config_dir_override {
        if config_dir.as_os_str().is_empty() {
            bail!("empty --config-dir path specified")
        }
        Ok(config_dir.clone())
    // Then check if the optional mount path provided by some subcommands is an EdenFS mount.
    // If it's provided and is a valid EdenFS mount, use the mounts config dir.
    } else if let Some(config_dir) = mount_path_override
        .as_ref()
        .and_then(|x| locate_eden_config_dir(x))
    {
        Ok(config_dir)
    // Then check if the current working directory is an EdenFS mount. If not, we should
    // default to the default config-dir location which varies by platform.
    } else {
        Ok(env::current_dir()
            .map_err(From::from)
            .and_then(|cwd| {
                locate_eden_config_dir(&cwd).ok_or_else(|| anyhow!("cwd is not in an eden mount"))
            })
            .unwrap_or(expand_path(DEFAULT_CONFIG_DIR)))
    }
}

pub fn get_etc_eden_dir(etc_eden_dir_override: &Option<PathBuf>) -> PathBuf {
    if let Some(etc_eden_dir) = etc_eden_dir_override {
        etc_eden_dir.clone()
    } else {
        DEFAULT_ETC_EDEN_DIR.into()
    }
}

pub fn get_home_dir(home_dir_override: &Option<PathBuf>) -> Option<PathBuf> {
    if let Some(home_dir) = home_dir_override {
        Some(home_dir.clone())
    } else {
        dirs::home_dir()
    }
}

/// Given a prefix and a list of paths, return a list of paths with the prefix prepended to each path.
///
/// If the prefix is None the paths are processed as-is.
/// All paths are post-processed with the provided function.
pub(crate) fn prefix_paths<F, T>(
    prefix: &Option<PathBuf>,
    paths: &Option<Vec<PathBuf>>,
    f: F,
) -> Option<Vec<T>>
where
    F: Fn(PathBuf) -> T,
{
    if let Some(prefix) = prefix {
        paths
            .as_ref()
            .map(|ps| ps.iter().map(|p| f(prefix.join(p))).collect::<Vec<_>>())
    } else {
        paths
            .as_ref()
            .map(|ps| ps.iter().map(|p| f(p.to_path_buf())).collect::<Vec<_>>())
    }
}

/// Given a prefix and a path string, return the path with the prefix removed.
///
/// If the prefix is None, the path is returned as-is.
pub fn strip_prefix_from_string(prefix: &Option<PathBuf>, path: String) -> String {
    if let Some(prefix) = prefix {
        let path = Path::new(&path);
        path.strip_prefix(prefix)
            .map_or(path, |stripped_path| stripped_path)
            .to_string_lossy()
            .to_string()
    } else {
        path
    }
}

pub(crate) fn strip_prefix_from_bytes(prefix: &Option<PathBuf>, path: &[u8]) -> Vec<u8> {
    if let Some(prefix) = prefix {
        let path = Path::new(std::str::from_utf8(path).expect("Failed to convert path to string"));
        path.strip_prefix(prefix)
            .map_or(path, |stripped_path| stripped_path)
            .to_string_lossy()
            .to_string()
            .into_bytes()
    } else {
        path.to_vec()
    }
}
