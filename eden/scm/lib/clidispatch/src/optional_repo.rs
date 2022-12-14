/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use anyhow::Result;
use configloader::config::ConfigSet;
use repo::errors;
use repo::repo::Repo;

use crate::global_flags::HgGlobalOpts;

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
    fn from_cwd(opts: &HgGlobalOpts, cwd: impl AsRef<Path>) -> Result<OptionalRepo> {
        if let Some((path, _)) = identity::sniff_root(&util::path::absolute(cwd)?)? {
            let repo = Repo::load(path, &opts.config, &opts.configfile)?;
            Ok(OptionalRepo::Some(repo))
        } else {
            Ok(OptionalRepo::None(configloader::hg::load(
                None,
                &opts.config,
                &opts.configfile,
            )?))
        }
    }

    /// Load the repo from the global --repository (or --repo, -R) flag.
    ///
    /// -R can be either a directory or a bundle file.
    pub fn from_global_opts(opts: &HgGlobalOpts, cwd: impl AsRef<Path>) -> Result<OptionalRepo> {
        let repository_path: &Path = opts.repository.as_ref();
        if repository_path.as_os_str().is_empty() {
            // --repo is not specified, only use cwd.
            return Self::from_cwd(opts, cwd);
        }

        let cwd = cwd.as_ref();
        let full_repository_path =
            if repository_path == Path::new(".") || repository_path == Path::new("") {
                cwd.to_path_buf()
            } else {
                cwd.join(repository_path)
            };
        if let Ok(path) = util::path::absolute(&full_repository_path) {
            if identity::sniff_dir(&path)?.is_some() {
                let repo = Repo::load(path, &opts.config, &opts.configfile)?;
                return Ok(OptionalRepo::Some(repo));
            } else if path.is_file() {
                // 'path' is a bundle path
                return Self::from_cwd(opts, cwd);
            }
        }
        Err(errors::RepoNotFound(repository_path.display().to_string()).into())
    }

    pub fn config_mut(&mut self) -> &mut ConfigSet {
        match self {
            OptionalRepo::Some(ref mut repo) => repo.config_mut(),
            OptionalRepo::None(ref mut config) => config,
        }
    }

    pub fn config(&self) -> &ConfigSet {
        match self {
            OptionalRepo::Some(ref repo) => repo.config(),
            OptionalRepo::None(ref config) => config,
        }
    }
}
