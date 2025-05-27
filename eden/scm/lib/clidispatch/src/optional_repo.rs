/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use configloader::hg::RepoInfo;
use configmodel::Config;
use gitcompat::init::maybe_init_inside_dotgit;
use repo::errors;
use repo::repo::Repo;

use crate::global_flags::HgGlobalOpts;
use crate::util::pinned_configs;

/// Either an optional [`Repo`] which owns a [`Arc<dyn Config>`], or a [`Arc<dyn Config>`]
/// without a repo.
pub enum OptionalRepo {
    Some(Repo),
    None(Arc<dyn Config>),
}

impl OptionalRepo {
    /// Optionally load a repo from the specified "current directory".
    ///
    /// Return None if there is no repo found from the current directory or its
    /// parent directories.
    fn from_cwd(opts: &HgGlobalOpts, cwd: impl AsRef<Path>) -> Result<OptionalRepo> {
        if let Some((path, ident)) = identity::sniff_root(&util::path::absolute(cwd)?)? {
            maybe_init_inside_dotgit(&path, ident)?;
            let repo = Repo::load(path, &pinned_configs(opts))?;
            Ok(OptionalRepo::Some(repo))
        } else {
            Ok(OptionalRepo::None(Arc::new(configloader::hg::load(
                RepoInfo::NoRepo,
                &pinned_configs(opts),
            )?)))
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
        if let Ok(path) = util::path::absolute(full_repository_path) {
            if let Some(ident) = identity::sniff_dir(&path)? {
                maybe_init_inside_dotgit(&path, ident)?;
                let repo = Repo::load(path, &pinned_configs(opts))?;
                return Ok(OptionalRepo::Some(repo));
            } else if path.is_file() {
                // 'path' is a bundle path
                return Self::from_cwd(opts, cwd);
            }
        }
        Err(errors::RepoNotFound(repository_path.display().to_string()).into())
    }

    pub fn config(&self) -> &Arc<dyn Config> {
        match self {
            OptionalRepo::Some(repo) => repo.config(),
            OptionalRepo::None(config) => config,
        }
    }

    pub fn repo_opt(&self) -> Option<&Repo> {
        match self {
            OptionalRepo::Some(repo) => Some(repo),
            OptionalRepo::None(_) => None,
        }
    }

    pub fn has_repo(&self) -> bool {
        matches!(self, OptionalRepo::Some(_))
    }
}
