/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Deref;

use crate::repo::RepoContext;

pub mod create_changeset;

/// Describes the permissions model that is being used to determine if a write is
/// permitted or not.
pub enum PermissionsModel {
    /// Writes are checked against the actions that a particular service may perform.
    ServiceIdentity(String),

    /// Any valid write is permitted.
    AllowAnyWrite,
}

pub struct RepoWriteContext {
    /// Repo that is being written to.
    repo: RepoContext,

    /// What checks to perform for the writes.
    #[allow(dead_code)]
    permissions_model: PermissionsModel,
}

impl Deref for RepoWriteContext {
    type Target = RepoContext;

    fn deref(&self) -> &RepoContext {
        &self.repo
    }
}

impl RepoWriteContext {
    pub(crate) fn new(repo: RepoContext, permissions_model: PermissionsModel) -> Self {
        Self {
            repo,
            permissions_model,
        }
    }
}
