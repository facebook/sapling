/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(feature = "fb")]
mod direct;
mod eden;
mod git;

use std::path::Path;

use clidispatch::ReqCtx;
use clidispatch::TermLogger;
use cmdutil::Result;
use repo::repo::Repo;
use types::HgId;
use worktree::Worktrees;

use crate::WorktreeOpts;

/// Creates and removes checkouts with EdenFS or Git, whichever backs the repo.
pub(crate) enum Checkouts<'a> {
    Eden(eden::EdenCheckouts<'a>),
    Git(git::GitCheckouts<'a>),
}

/// Clone options captured before the pre-add hooks run.
pub(crate) enum AddSource<'a> {
    Eden(eden::AddSource<'a>),
    Git(git::GitCheckouts<'a>),
}

/// Source state needed to materialize a new checkout.
pub(crate) enum PreparedAdd<'a> {
    Eden(eden::PreparedAdd<'a>),
    Git(git::PreparedAdd<'a>),
}

impl<'a> Checkouts<'a> {
    pub(crate) fn new(repo: &'a Repo, worktrees: &'a Worktrees) -> Self {
        match worktrees.git() {
            Some(git) => Self::Git(git::GitCheckouts::new(repo, git)),
            None => Self::Eden(eden::EdenCheckouts::new(repo)),
        }
    }

    /// Whether `add --snapshot` can carry working copy changes into a new checkout.
    pub(crate) fn supports_snapshot(&self) -> bool {
        matches!(self, Self::Eden(_))
    }

    /// The command that cleans up what a failed add left at `dest`, if the
    /// error doesn't already say.
    pub(crate) fn cleanup_command(&self, dest: &Path) -> Option<String> {
        match self {
            Self::Eden(_) => Some(format!("eden rm {}", dest.display())),
            Self::Git(_) => None,
        }
    }

    /// Remove one checkout. The caller holds its path lock. Returns false if the
    /// checkout was already missing on disk.
    pub(crate) fn remove(&self, logger: &TermLogger, path: &Path) -> Result<bool> {
        match self {
            Self::Eden(eden) => eden.remove(logger, path),
            Self::Git(_) => anyhow::bail!("worktree remove is not supported in Git repositories"),
        }
    }

    /// Whether the checkout at `path` has modified or untracked files.
    pub(crate) fn has_changes(&self, ctx: &ReqCtx<WorktreeOpts>, path: &Path) -> Result<bool> {
        match self {
            Self::Eden(eden) => eden.has_changes(ctx, path),
            Self::Git(_) => anyhow::bail!(
                "checking for modified or untracked files is not supported in Git repositories"
            ),
        }
    }

    pub(crate) fn add_source(&self) -> Result<AddSource<'a>> {
        match self {
            Self::Eden(eden) => Ok(AddSource::Eden(eden.add_source()?)),
            Self::Git(git) => Ok(AddSource::Git(*git)),
        }
    }
}

impl<'a> AddSource<'a> {
    /// Capture the source checkout state. The caller holds the source path lock.
    pub(crate) fn capture(self, requested_target: Option<HgId>) -> Result<PreparedAdd<'a>> {
        match self {
            Self::Eden(source) => Ok(PreparedAdd::Eden(source.capture(requested_target)?)),
            Self::Git(git) => Ok(PreparedAdd::Git(git.capture(requested_target)?)),
        }
    }
}

impl PreparedAdd<'_> {
    /// Create and configure the checkout. The caller holds the destination path lock.
    pub(crate) fn create(self, dest: &Path) -> Result<()> {
        match self {
            Self::Eden(prepared) => prepared.create(dest),
            Self::Git(prepared) => prepared.create(dest),
        }
    }
}

fn resolve_target(repo: &Repo, requested_target: Option<HgId>) -> Result<Option<HgId>> {
    let parents =
        workingcopy::fast_path_wdir_parents_with_config(repo.path(), repo.ident(), repo.config())?;
    Ok(requested_target.or_else(|| parents.p1().copied()))
}
