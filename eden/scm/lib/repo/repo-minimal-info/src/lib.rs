/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub mod constants;
pub mod requirements;

use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use anyhow::bail;
use fs_err as fs;
use identity::dotgit::follow_dotgit_path;
use identity::dotgit::resolve_git_common_dir;
pub use requirements::Requirements;

/// RepoMinimalInfo contains:
/// - Identity.
/// - Shared path and shared identity.
/// - Repo requirements.
///
/// It can be useful by the config loader to decide extra config
/// per requirement.
///
/// Sapling repo example:
///
/// ```text
/// /repos/wc/.sl/sharedpath: /repos/backing/.sl
///
/// path:               /repos/wc
/// shared_path:        /repos/backing
/// dot_hg_path:        /repos/wc/.sl
/// shared_dot_hg_path: /repos/backing/.sl
/// store_path:         /repos/backing/.sl/store
/// ```
///
/// Git repo (worktree) example:
///
/// ```text
/// /repos/topic/.git: gitdir: /repos/main/.git/worktrees/topic
/// /repos/main/.git/worktrees/topic/commondir: ../..
///
/// path:               /repos/topic
/// shared_path:        /repos/topic (no direct Git equivalent)
/// GIT_DIR:            /repos/main/.git/worktrees/topic
/// GIT_COMMON_DIR:     /repos/main/.git
/// dot_hg_path:        /repos/main/.git/worktrees/topic/sl
/// shared_dot_hg_path: /repos/main/.git/sl
/// store_path:         /repos/main/.git/sl/store
/// ```
#[derive(Clone)]
pub struct RepoMinimalInfo {
    pub path: PathBuf,
    pub ident: identity::Identity,
    pub shared_path: PathBuf,
    pub shared_ident: identity::Identity,
    pub store_path: PathBuf,
    pub dot_hg_path: PathBuf,
    pub shared_dot_hg_path: PathBuf,
    pub requirements: Requirements,
    pub store_requirements: Requirements,
}

impl RepoMinimalInfo {
    /// Load the minimal info from a given path.
    ///
    /// If there is no supported repo at the given path, return `None`.
    /// Does not look at ancestor directories.
    pub fn from_repo_root(mut path: PathBuf) -> Result<Self> {
        if !path.is_absolute() {
            path = fs::canonicalize(path)?;
        }
        let ident = match identity::sniff_dir(&path)? {
            Some(ident) => ident,
            None => bail!("repository {} not found!", path.display()),
        };

        let (dot_hg_path, git_common_dir) = if ident.is_dot_git() {
            // follow_dotgit_path() returns the per-worktree GIT_DIR.
            let dot_git_path = follow_dotgit_path(path.join(".git"));
            let git_common_dir = resolve_git_common_dir(&dot_git_path)?;
            (dot_git_path.join("sl"), Some(git_common_dir))
        } else {
            (path.join(ident.dot_dir()), None)
        };

        let (shared_dot_hg_path, shared_path, shared_ident) = match git_common_dir {
            Some(git_common_dir) => (git_common_dir.join("sl"), path.clone(), ident),
            None => match read_sharedpath(&dot_hg_path)? {
                Some((path, ident)) => (path.join(ident.dot_dir()), path, ident),
                None => (dot_hg_path.clone(), path.clone(), ident),
            },
        };
        let store_path = shared_dot_hg_path.join("store");

        let requirements = Requirements::load_repo_requirements(&dot_hg_path)?;
        let store_requirements = Requirements::load_store_requirements(&store_path)?;

        let info = Self {
            path,
            ident,
            shared_path,
            shared_ident,
            store_path,
            dot_hg_path,
            shared_dot_hg_path,
            requirements,
            store_requirements,
        };

        Ok(info)
    }
}

// Git-specific path helpers.
impl RepoMinimalInfo {
    /// Returns `$GIT_DIR` for git repos.
    pub fn git_dir(&self) -> Option<&Path> {
        if self.ident.is_dot_git() {
            self.dot_hg_path.parent()
        } else {
            None
        }
    }

    /// Returns `$GIT_COMMON_DIR` for git repos.
    pub fn git_common_dir(&self) -> Option<&Path> {
        if self.ident.is_dot_git() {
            self.shared_dot_hg_path.parent()
        } else {
            None
        }
    }
}

pub fn read_sharedpath(dot_path: &Path) -> Result<Option<(PathBuf, identity::Identity)>> {
    let sharedpath = fs::read_to_string(dot_path.join("sharedpath"))
        .ok()
        .map(PathBuf::from)
        .and_then(|p| Some(PathBuf::from(p.parent()?)));

    if let Some(mut possible_path) = sharedpath {
        // sharedpath can be relative to our dot dir.
        possible_path = dot_path.join(possible_path);

        return match identity::sniff_dir(&possible_path)? {
            Some(ident) => Ok(Some((possible_path, ident))),
            None => bail!(
                "sharedpath points to nonexistent directory {}!",
                possible_path.display()
            ),
        };
    }

    Ok(None)
}
