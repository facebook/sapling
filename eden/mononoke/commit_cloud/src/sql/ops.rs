/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use context::CoreContext;
use sql_ext::SqlConnections;
use sql_ext::Transaction;

use crate::ctx::CommitCloudContext;
pub struct SqlCommitCloud {
    pub connections: SqlConnections,
}

impl SqlCommitCloud {
    pub fn new(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

#[async_trait]
pub trait Get<T = Self> {
    async fn get(
        &self,
        ctx: &CoreContext,
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<Vec<T>>;
}

#[async_trait]
pub trait GetAsMap<T = Self> {
    async fn get_as_map(
        &self,
        ctx: &CoreContext,
        reponame: String,
        workspace: String,
    ) -> anyhow::Result<T>;
}

#[async_trait]
pub trait GenericGet<T = Self> {
    type GetArgs;
    type GetOutput;
    async fn get(
        &self,
        ctx: &CoreContext,
        reponame: String,
        workspace: String,
        args: Self::GetArgs,
    ) -> anyhow::Result<Vec<Self::GetOutput>>;
}

#[async_trait]
pub trait Insert<T = Self> {
    async fn insert(
        &self,
        txn: Transaction,
        _ctx: &CoreContext,
        reponame: String,
        workspace: String,
        data: T,
    ) -> anyhow::Result<Transaction>;
}

#[async_trait]
pub trait Update<T = Self> {
    type UpdateArgs;
    async fn update(
        &self,
        txn: Transaction,
        _ctx: &CoreContext,
        cc_ctx: CommitCloudContext,
        args: Self::UpdateArgs,
    ) -> anyhow::Result<(Transaction, u64)>;
}

#[async_trait]
pub trait Delete<T = Self> {
    type DeleteArgs;
    async fn delete(
        &self,
        txn: Transaction,
        _ctx: &CoreContext,
        reponame: String,
        workspace: String,
        args: Self::DeleteArgs,
    ) -> anyhow::Result<Transaction>;
}

trait SqlCommitCloudOps<T> = Get<T> + Update<T> + Insert<T> + Delete<T>;
trait ImmutableSqlCommitCloudOps<T> = Get<T> + Update<T> + Insert<T>;
