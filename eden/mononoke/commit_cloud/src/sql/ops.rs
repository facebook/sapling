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
}

impl SqlCommitCloud {
    pub fn new(connections: SqlConnections) -> Self {
        Self { connections }
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
