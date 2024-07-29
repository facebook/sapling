/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use commit_cloud_helpers::sanity_check_workspace_name;

#[derive(Debug, Clone)]
pub struct CommitCloudContext {
    pub workspace: String,
    pub reponame: String,
}

impl CommitCloudContext {
    pub fn new(workspace: &str, reponame: &str) -> anyhow::Result<Self> {
        if workspace.is_empty() || reponame.is_empty() {
            return Err(anyhow::anyhow!(
                "'commit cloud' failed: empty reponame or workspace"
            ));
        }
        Ok(Self {
            workspace: workspace.to_owned(),
            reponame: reponame.to_owned(),
        })
    }

    pub fn check_workspace_name(&self) -> anyhow::Result<()> {
        if !sanity_check_workspace_name(&self.workspace) {
            return Err(anyhow::anyhow!(
                "'commit cloud' failed: creating a new workspace with name '{}' is not allowed",
                self.workspace
            ));
        }
        Ok(())
    }
}
