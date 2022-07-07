/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Deref;

use crate::repo::RepoContext;

pub mod create_changeset;
pub mod set_git_mapping;

pub struct RepoDraftContext {
    /// Repo that is being written to.
    repo: RepoContext,
}

impl Deref for RepoDraftContext {
    type Target = RepoContext;

    fn deref(&self) -> &RepoContext {
        &self.repo
    }
}

impl RepoDraftContext {
    pub(crate) fn new(repo: RepoContext) -> Self {
        Self { repo }
    }

    pub fn repo(&self) -> &RepoContext {
        &self.repo
    }
}
