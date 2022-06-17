/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ops::Deref;

use crate::errors::MononokeError;
use crate::permissions::WritePermissionsModel;
use crate::repo::RepoContext;

pub mod create_changeset;
pub mod set_git_mapping;

pub struct RepoDraftContext {
    /// Repo that is being written to.
    repo: RepoContext,

    /// What checks to perform for the writes.
    permissions_model: WritePermissionsModel,
}

impl Deref for RepoDraftContext {
    type Target = RepoContext;

    fn deref(&self) -> &RepoContext {
        &self.repo
    }
}

impl RepoDraftContext {
    pub(crate) fn new(repo: RepoContext, permissions_model: WritePermissionsModel) -> Self {
        Self {
            repo,
            permissions_model,
        }
    }

    pub fn repo(&self) -> &RepoContext {
        &self.repo
    }

    fn check_method_permitted(&self, method: &str) -> Result<(), MononokeError> {
        match &self.permissions_model {
            WritePermissionsModel::ServiceIdentity(service_identity) => {
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
            WritePermissionsModel::AllowAnyWrite => {}
        }
        Ok(())
    }
}
