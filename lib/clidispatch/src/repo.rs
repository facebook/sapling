// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use crate::errors;
use configparser::{config::ConfigSet, hg::ConfigSetHgExt};
use failure::Fallible;
use std::fs;
use std::path::Path;
use std::path::PathBuf;

pub struct Repo {
    path: PathBuf,
    config: ConfigSet,
    bundle_path: Option<PathBuf>,
    shared_path: Option<PathBuf>,
}

/// Either an optional [`Repo`] which owns a [`ConfigSet`], or a [`ConfigSet`]
/// without a repo.
pub enum OptionalRepo {
    Some(Repo),
    None(ConfigSet),
}

impl OptionalRepo {
    /// Optionally load a repo from the specified "current directory".
    ///
    /// Return None if there is no repo found from the current directory or its
    /// parent directories.
    pub fn from_cwd(cwd: impl AsRef<Path>, config: ConfigSet) -> Fallible<OptionalRepo> {
        if let Some(path) = find_hg_repo_root(&util::path::absolute(cwd)?) {
            let repo = Repo::from_raw_path(path, config)?;
            Ok(OptionalRepo::Some(repo))
        } else {
            Ok(OptionalRepo::None(config))
        }
    }

    /// Load the repo from a --repository (or --repo, -R) flag.
    ///
    /// The path can be either a directory or a bundle file.
    pub fn from_repository_path_and_cwd(
        repository_path: impl AsRef<Path>,
        cwd: impl AsRef<Path>,
        config: ConfigSet,
    ) -> Fallible<OptionalRepo> {
        let repository_path = repository_path.as_ref();
        if repository_path.as_os_str().is_empty() {
            // --repo is not specified, only use cwd.
            return Self::from_cwd(cwd, config);
        }

        if let Ok(path) = util::path::absolute(repository_path) {
            if path.join(".hg").is_dir() {
                // `path` is a directory with `.hg`.
                let repo = Repo::from_raw_path(path, config)?;
                return Ok(OptionalRepo::Some(repo));
            } else if path.is_file() {
                // 'path' is a bundle path
                if let OptionalRepo::Some(mut repo) = Self::from_cwd(cwd, config)? {
                    repo.bundle_path = Some(path);
                    return Ok(OptionalRepo::Some(repo));
                }
            }
        }
        Err(errors::RepoNotFound(repository_path.to_string_lossy().to_string()).into())
    }

    pub fn config_mut(&mut self) -> &mut ConfigSet {
        match self {
            OptionalRepo::Some(ref mut repo) => &mut repo.config,
            OptionalRepo::None(ref mut config) => config,
        }
    }

    pub fn config(&mut self) -> &ConfigSet {
        match self {
            OptionalRepo::Some(ref repo) => &repo.config,
            OptionalRepo::None(ref config) => config,
        }
    }
}

impl Repo {
    /// Load the repo from explicit path.
    ///
    /// Load repo configurations.
    fn from_raw_path<P>(path: P, mut config: ConfigSet) -> Fallible<Self>
    where
        P: Into<PathBuf>,
    {
        let path = path.into();
        assert!(path.is_absolute());
        let mut errors = config.load_hgrc(path.join(".hg/hgrc"), "repository");
        if let Some(error) = errors.pop() {
            Err(error.into())
        } else {
            let shared_path = read_sharedpath(&path)?;
            Ok(Repo {
                path,
                config,
                bundle_path: None,
                shared_path,
            })
        }
    }

    pub fn shared_path(&self) -> Option<&Path> {
        match &self.shared_path {
            Some(path) => Some(path),
            None => None,
        }
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub fn config(&self) -> &ConfigSet {
        &self.config
    }
}

fn find_hg_repo_root(current_path: &Path) -> Option<PathBuf> {
    assert!(current_path.is_absolute());
    if current_path.join(".hg").is_dir() {
        Some(current_path.to_path_buf())
    } else if let Some(parent) = current_path.parent() {
        find_hg_repo_root(parent)
    } else {
        None
    }
}

fn read_sharedpath(path: &Path) -> Fallible<Option<PathBuf>> {
    let mut sharedpath = fs::read_to_string(path.join(".hg/sharedpath"))
        .ok()
        .map(|s| PathBuf::from(s))
        .and_then(|p| Some(PathBuf::from(p.parent()?)));

    if let Some(possible_path) = sharedpath {
        if possible_path.is_absolute() && !possible_path.is_dir() {
            return Err(errors::InvalidSharedPath(
                possible_path.join(".hg").to_string_lossy().to_string(),
            )
            .into());
        } else if possible_path.is_absolute() {
            sharedpath = Some(possible_path)
        } else {
            // join relative path from the REPO/.hg path
            let new_possible = path.join(".hg").join(possible_path);
            if !new_possible.join(".hg").exists() {
                return Err(
                    errors::InvalidSharedPath(new_possible.to_string_lossy().to_string()).into(),
                );
            }
            sharedpath = Some(new_possible)
        }
    }
    Ok(sharedpath)
}
