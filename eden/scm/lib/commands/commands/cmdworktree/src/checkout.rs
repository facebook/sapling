/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(feature = "fb")]
mod direct;
mod eden;

use std::path::Path;

use clidispatch::TermLogger;
use cmdutil::Result;
use repo::repo::Repo;
use types::HgId;

/// Creates and removes checkouts with EdenFS.
pub(crate) enum Checkouts<'a> {
    Eden(eden::EdenCheckouts<'a>),
}

/// Clone options captured before the pre-add hooks run.
pub(crate) enum AddSource<'a> {
    Eden(eden::AddSource<'a>),
}

/// Source state needed to materialize a new checkout.
pub(crate) enum PreparedAdd<'a> {
    Eden(eden::PreparedAdd<'a>),
}

impl<'a> Checkouts<'a> {
    pub(crate) fn new(repo: &'a Repo) -> Self {
        Self::Eden(eden::EdenCheckouts::new(repo))
    }

    pub(crate) fn add_source(&self) -> Result<AddSource<'a>> {
        match self {
            Self::Eden(eden) => Ok(AddSource::Eden(eden.add_source()?)),
        }
    }
}

impl<'a> AddSource<'a> {
    /// Capture the source checkout state. The caller holds the source path lock.
    pub(crate) fn capture(self, requested_target: Option<HgId>) -> Result<PreparedAdd<'a>> {
        match self {
            Self::Eden(source) => Ok(PreparedAdd::Eden(source.capture(requested_target)?)),
        }
    }
}

impl PreparedAdd<'_> {
    /// Create and configure the checkout. The caller holds the destination path lock.
    pub(crate) fn create(self, dest: &Path) -> Result<()> {
        match self {
            Self::Eden(prepared) => prepared.create(dest),
        }
    }
}

/// Remove one checkout. The caller holds its path lock. Returns false if the
/// checkout was already missing on disk.
pub(crate) fn remove_checkout(logger: &TermLogger, repo: &Repo, path: &Path) -> Result<bool> {
    eden::remove_checkout(logger, repo, path)
}

fn resolve_target(repo: &Repo, requested_target: Option<HgId>) -> Result<Option<HgId>> {
    let parents =
        workingcopy::fast_path_wdir_parents_with_config(repo.path(), repo.ident(), repo.config())?;
    Ok(requested_target.or_else(|| parents.p1().copied()))
}
