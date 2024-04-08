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
pub trait BasicOps<T = Self> {
    type ExtraArgs;

    async fn get(
        &self,
        reponame: String,
        workspace: String,
        extra_args: Self::ExtraArgs,
    ) -> anyhow::Result<Vec<T>>;
    async fn insert(
        &self,
        reponame: String,
        workspace: String,
        data: T,
        extra_args: Self::ExtraArgs,
    ) -> anyhow::Result<bool>;
    async fn delete(
        &self,
        reponame: String,
        workspace: String,
        extra_args: Self::ExtraArgs,
    ) -> anyhow::Result<bool>;
    async fn update(
        &self,
        reponame: String,
        workspace: String,
        extra_args: Self::ExtraArgs,
    ) -> anyhow::Result<bool>;
}
