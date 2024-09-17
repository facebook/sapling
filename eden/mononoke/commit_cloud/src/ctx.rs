/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::ensure;
use commit_cloud_helpers::sanity_check_workspace_name;
use commit_cloud_types::CommitCloudError;
use commit_cloud_types::CommitCloudUserError;
use permission_checker::MononokeIdentity;

#[derive(Debug, Clone)]
pub struct CommitCloudContext {
    pub workspace: String,
    pub reponame: String,
    pub owner: Option<MononokeIdentity>,
}

impl CommitCloudContext {
    pub fn new(workspace: &str, reponame: &str) -> anyhow::Result<Self> {
        ensure!(
            !workspace.is_empty() && !reponame.is_empty(),
            "'commit cloud' failed: empty reponame or workspace"
        );

        Ok(Self {
            workspace: workspace.to_owned(),
            reponame: reponame.to_owned(),
            owner: None,
        })
    }

    pub fn check_workspace_name(&self) -> Result<(), CommitCloudError> {
        if !sanity_check_workspace_name(&self.workspace) {
            return Err(CommitCloudUserError::WorkspaceNameNotAllowed(
                self.workspace.clone(),
                self.reponame.clone(),
            )
            .into());
        }

        Ok(())
    }

    pub fn set_owner(&mut self, owner: Option<MononokeIdentity>) {
        self.owner = owner;
    }
}
