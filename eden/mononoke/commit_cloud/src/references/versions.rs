/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::Timestamp;

use crate::Get;
use crate::SqlCommitCloud;

#[derive(Clone, Debug, PartialEq)]
pub struct WorkspaceVersion {
    pub workspace: String,
    pub version: u64,
    pub timestamp: Timestamp,
    pub archived: bool,
}

impl WorkspaceVersion {
    pub async fn fetch_from_db(
        sql: &SqlCommitCloud,
        workspace: &str,
        reponame: &str,
    ) -> anyhow::Result<Option<Self>> {
        Get::<WorkspaceVersion>::get(sql, reponame.to_owned(), workspace.to_owned())
            .await
            .map(|versions| versions.into_iter().next())
    }
}
