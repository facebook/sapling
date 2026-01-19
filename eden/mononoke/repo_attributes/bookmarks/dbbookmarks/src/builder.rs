/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;

use crate::store::SqlBookmarks;

#[derive(Clone)]
pub struct SqlBookmarksBuilder {
    pub(crate) connections: SqlConnections,
}

impl SqlConstruct for SqlBookmarksBuilder {
    const LABEL: &'static str = "bookmarks";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-bookmarks.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlBookmarksBuilder {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&RemoteDatabaseConfig> {
        Some(&remote.bookmarks)
    }
    fn oss_remote_database_config(
        remote: &OssRemoteMetadataDatabaseConfig,
    ) -> Option<&OssRemoteDatabaseConfig> {
        Some(&remote.bookmarks)
    }
}

impl SqlBookmarksBuilder {
    pub fn with_repo_id(self, repo_id: RepositoryId) -> SqlBookmarks {
        SqlBookmarks::new(repo_id, self.connections)
    }
}
