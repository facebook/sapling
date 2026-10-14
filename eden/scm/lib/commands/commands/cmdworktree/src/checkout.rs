/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(feature = "fb")]
mod direct;

use std::collections::HashSet;
use std::path::Path;

use clidispatch::TermLogger;
use cmdutil::ConfigExt;
use cmdutil::Result;
use configmodel::Text;
use repo::repo::Repo;
use types::HgId;

#[cfg(feature = "fb")]
use self::direct::run_direct_eden_remove;

/// Clone options captured before the pre-add hooks run.
pub(crate) struct AddSource<'a> {
    repo: &'a Repo,
    clone_filters: Option<HashSet<Text>>,
}

/// Source state needed to materialize a new checkout.
pub(crate) struct PreparedAdd<'a> {
    repo: &'a Repo,
    target: Option<HgId>,
    clone_filters: Option<HashSet<Text>>,
    sparse_config: Option<Vec<u8>>,
    user_config: Option<String>,
}

impl<'a> AddSource<'a> {
    pub(crate) fn new(repo: &'a Repo) -> Result<Self> {
        // Replicate the source repo's scm type and active filters.
        // When edensparse is in requirements, the backing store should be filteredhg
        // (even with no filter paths configured). Otherwise the backing store is hg.
        // Read active filter paths from the source repo's .sl/sparse file.
        let clone_filters = repo
            .requirements
            .contains("edensparse")
            .then(|| -> anyhow::Result<_> {
                let paths = filters::util::read_filter_config(repo.dot_hg_path())?
                    .map(|paths| paths.into_iter().map(|p| p.into_string().into()).collect())
                    .unwrap_or_default();
                Ok(paths)
            })
            .transpose()?;
        Ok(Self {
            repo,
            clone_filters,
        })
    }

    /// Capture the source checkout state. The caller holds the source path lock.
    pub(crate) fn capture(self, requested_target: Option<HgId>) -> Result<PreparedAdd<'a>> {
        let repo = self.repo;
        let source_client_dir = edenfs_client::get_client_dir(repo.path())?;
        let parents = workingcopy::fast_path_wdir_parents_with_config(
            repo.path(),
            repo.ident(),
            repo.config(),
        )?;
        Ok(PreparedAdd {
            repo,
            target: requested_target.or_else(|| parents.p1().copied()),
            clone_filters: self.clone_filters,
            sparse_config: clone::snapshot_sparse_config(repo.dot_hg_path())?,
            user_config: clone::snapshot_eden_user_config(&source_client_dir)?,
        })
    }
}

impl PreparedAdd<'_> {
    /// Create and configure the checkout. The caller holds the destination path lock.
    pub(crate) fn create(self, dest: &Path) -> Result<()> {
        let repo = self.repo;
        clone::eden_clone(repo, dest, self.target, self.clone_filters)?;
        if let Some(config) = self.sparse_config {
            clone::write_sparse_config(&config, &dest.join(repo.ident().dot_dir()))?;
        }
        if let Some(config) = self.user_config {
            clone::apply_eden_user_config_snapshot(repo.config().as_ref(), &config, dest)?;
        }
        Ok(())
    }
}

/// Remove one checkout. The caller holds its path lock. Returns false if the
/// checkout was already missing on disk.
pub(crate) fn remove_checkout(logger: &TermLogger, repo: &Repo, path: &Path) -> Result<bool> {
    if !path.exists() {
        return Ok(false);
    }
    run_eden_remove(logger, repo, path)?;
    Ok(true)
}

fn run_eden_remove(logger: &TermLogger, repo: &Repo, path: &Path) -> Result<()> {
    if repo
        .config()
        .get_or("worktree", "direct-eden-removal", || cfg!(feature = "fb"))?
    {
        run_direct_eden_remove(logger, repo, path)
    } else {
        edenfs_client::run_eden_remove(repo.config().as_ref(), path)
    }
}

#[cfg(not(feature = "fb"))]
fn run_direct_eden_remove(_logger: &TermLogger, _repo: &Repo, _path: &Path) -> Result<()> {
    clidispatch::abort!("worktree.direct-eden-removal is not supported in this build");
}
