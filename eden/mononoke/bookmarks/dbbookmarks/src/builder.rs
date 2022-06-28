/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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

impl SqlConstructFromMetadataDatabaseConfig for SqlBookmarksBuilder {}

impl SqlBookmarksBuilder {
    pub fn with_repo_id(self, repo_id: RepositoryId) -> SqlBookmarks {
        SqlBookmarks::new(repo_id, self.connections)
    }
}
