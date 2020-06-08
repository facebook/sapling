/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Deref;

use crate::repo::RepoContext;

pub mod create_changeset;

pub struct RepoWriteContext {
    repo: RepoContext,
}

impl Deref for RepoWriteContext {
    type Target = RepoContext;

    fn deref(&self) -> &RepoContext {
        &self.repo
    }
}

impl RepoWriteContext {
    pub(crate) fn new(repo: RepoContext) -> Self {
        Self { repo }
    }
}
