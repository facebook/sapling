/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use sql_ext::SqlConnections;

pub struct SqlCommitCloud {
    #[allow(unused)]
    pub(crate) connections: SqlConnections,
    // Commit cloud has three databases in mononoke:
    // 1. xdb.commit_cloud (prod) This is a mysql db used in prod
    // 2. sqlite db (test) This is created from sqlite-commit-cloud.sql. Used for unit tests.
    // 3. mock mysql db (test) This is used in integration tests, it's never queried or populated,
    /// just there to avoid a clash between "bookmarks" tables
    #[allow(unused)]
    pub(crate) uses_mysql: bool,
}

impl SqlCommitCloud {
    pub fn new(connections: SqlConnections, uses_mysql: bool) -> Self {
        Self {
            connections,
            uses_mysql,
        }
    }
}

#[async_trait]
pub trait Get<T = Self> {
    async fn get(&self, reponame: String, workspace: String) -> anyhow::Result<Vec<T>>;
}
#[async_trait]
pub trait GenericGet<T = Self> {
    type GetArgs;
    type GetOutput;
    async fn get(
        &self,
        reponame: String,
        workspace: String,
        args: Self::GetArgs,
    ) -> anyhow::Result<Vec<Self::GetOutput>>;
}

#[async_trait]
pub trait Insert<T = Self> {
    async fn insert(&self, reponame: String, workspace: String, data: T) -> anyhow::Result<()>;
}

#[async_trait]
pub trait Update<T = Self> {
    type UpdateArgs;
    async fn update(
        &self,
        reponame: String,
        workspace: String,
        args: Self::UpdateArgs,
    ) -> anyhow::Result<()>;
}

#[async_trait]
pub trait Delete<T = Self> {
    type DeleteArgs;
    async fn delete(
        &self,
        reponame: String,
        workspace: String,
        args: Self::DeleteArgs,
    ) -> anyhow::Result<()>;
}

trait SqlCommitCloudOps<T> = Get<T> + Update<T> + Insert<T> + Delete<T>;
trait ImmutableSqlCommitCloudOps<T> = Get<T> + Update<T> + Insert<T>;
