/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Deref;

use crate::errors::MononokeError;
use crate::repo::RepoContext;

pub mod create_bookmark;
pub mod delete_bookmark;
pub mod land_stack;
pub mod move_bookmark;

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

    fn check_method_permitted(&self, method: &str) -> Result<(), MononokeError> {
        match &self.permissions_model {
            PermissionsModel::ServiceIdentity(service_identity) => {
                if !self
                    .config()
                    .source_control_service
                    .service_write_method_permitted(service_identity, method)
                {
                    return Err(MononokeError::ServiceRestricted {
                        service_identity: service_identity.to_string(),
                        action: format!("call method {}", method),
                        reponame: self.name().to_string(),
                    });
                }
            }
            PermissionsModel::AllowAnyWrite => {}
        }
        Ok(())
    }
}
