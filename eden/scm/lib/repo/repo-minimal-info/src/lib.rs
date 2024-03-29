/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod constants;
pub mod requirements;

use std::path::Path;
use std::path::PathBuf;

use anyhow::bail;
use anyhow::Result;
use constants::SUPPORTED_DEFAULT_REQUIREMENTS;
use constants::SUPPORTED_STORE_REQUIREMENTS;
use fs_err as fs;
pub use requirements::Requirements;

/// RepoMinimalInfo contains:
/// - Identity.
/// - Shared path and shared identity.
/// - Repo requirements.
///
/// It can be useful by the config loader to decide extra config
/// per requirement.
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

        let dot_hg_path = path.join(ident.dot_dir());

        let (shared_path, shared_ident) = match read_sharedpath(&dot_hg_path)? {
            Some((path, ident)) => (path, ident),
            None => (path.clone(), ident.clone()),
        };
        let shared_dot_hg_path = shared_path.join(shared_ident.dot_dir());
        let store_path = shared_dot_hg_path.join("store");

        let requirements = Requirements::open(
            &dot_hg_path.join("requires"),
            &SUPPORTED_DEFAULT_REQUIREMENTS,
        )?;
        let store_requirements =
            Requirements::open(&store_path.join("requires"), &SUPPORTED_STORE_REQUIREMENTS)?;

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
