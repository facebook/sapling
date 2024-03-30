/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use sql_construct::SqlConstruct;
use sql_ext::SqlConnections;

use crate::SqlCommitCloud;
pub struct SqlCommitCloudBuilder {
    #[allow(unused)]
    pub(crate) connections: SqlConnections,
}

impl SqlConstruct for SqlCommitCloudBuilder {
    const LABEL: &'static str = "commit_cloud";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-commit-cloud.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlCommitCloudBuilder {
    pub fn new(self) -> SqlCommitCloud {
        SqlCommitCloud::new(self.connections)
    }
}
