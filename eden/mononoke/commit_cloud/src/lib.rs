/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]
pub mod sql;
pub mod workspace;

use crate::sql::ops::SqlCommitCloud;
use crate::sql::versions::WorkspaceVersion;
#[facet::facet]
pub struct CommitCloud {
    pub storage: SqlCommitCloud,
}

impl CommitCloud {
    pub async fn get_workspace(
        &self,
        workspace: &str,
        reponame: &str,
    ) -> anyhow::Result<Vec<WorkspaceVersion>> {
        use crate::sql::ops::Get;
        let workspace: anyhow::Result<Vec<WorkspaceVersion>> = self
            .storage
            .get(reponame.to_owned(), workspace.to_owned())
            .await;
        workspace
    }
}
