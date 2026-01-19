/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Context;
use anyhow::Error;
use anyhow::anyhow;
use async_trait::async_trait;
use context::CoreContext;
use maplit::hashmap;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::Connection;
use sql_ext::SqlConnections;
use sql_ext::Transaction;
use sql_ext::mononoke_queries;

const DEFAULT_DB_MSG: &str = "Repo is locked in DB";

#[derive(Eq, PartialEq, Debug)]
pub enum RepoLockState {
    Locked(String),
    Unlocked,
}

impl RepoLockState {
    /// Returns true if the repo is locked, false otherwise.
    pub fn is_locked(&self) -> bool {
        match self {
            RepoLockState::Locked(_) => true,
            RepoLockState::Unlocked => false,
        }
    }
}

#[facet::facet]
#[async_trait]
pub trait RepoLock: Send + Sync {
    /// Check whether a repo is locked, which will prevent new commits being pushed.
    async fn check_repo_lock(&self, ctx: &CoreContext) -> Result<RepoLockState, Error>;
    async fn all_repos_lock(
        &self,
        ctx: &CoreContext,
    ) -> Result<HashMap<RepositoryId, RepoLockState>, Error>;
    /// Lock a repo to prevent pushes. This method returns Ok(true) if the repo wasn't previously
    /// locked, Ok(false) if it was and Err(_) if there is an error modifying the lock status.
    async fn set_repo_lock(
        &self,
        ctx: &CoreContext,
        lock_state: RepoLockState,
    ) -> Result<bool, Error>;
}

mononoke_queries! {
    write SetRepoLockStatus(repo_id: RepositoryId, state: u8, reason: Option<&str>) {
        none,
        mysql("INSERT INTO repo_lock (repo_id, state, reason)
               VALUES ({repo_id}, {state}, {reason})
               ON DUPLICATE KEY UPDATE state = {state}, reason = {reason}")

        sqlite("INSERT OR REPLACE INTO repo_lock (repo_id, state, reason)
                VALUES ({repo_id}, {state}, {reason})")
    }

    read GetRepoLockStatus(repo_id: RepositoryId) -> (u8, Option<String>) {
        "SELECT state, reason FROM repo_lock
        WHERE repo_id = {repo_id}"
    }

    read AllReposLockStatus() -> (RepositoryId, u8, Option<String>) {
        "SELECT repo_id, state, reason FROM repo_lock"
    }
}

#[derive(Debug, Clone)]
pub struct SqlRepoLock {
    write_connection: Connection,
    read_connection: Connection,
}

impl SqlConstruct for SqlRepoLock {
    const LABEL: &'static str = "repo-lock";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-repo-lock.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            write_connection: connections.write_connection,
            read_connection: connections.read_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlRepoLock {
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

fn convert_sql_state((state, reason): &(u8, Option<String>)) -> Result<RepoLockState, Error> {
    match state {
        0 => Ok(RepoLockState::Unlocked),
        1 => Ok(RepoLockState::Locked(
            reason.clone().unwrap_or_else(|| DEFAULT_DB_MSG.to_string()),
        )),
        _ => Err(anyhow!("Invalid repo lock state: {}", state)),
    }
}

#[derive(Clone, Copy)]
pub struct TransactionRepoLock {
    repo_id: RepositoryId,
}

impl TransactionRepoLock {
    pub fn new(repo_id: RepositoryId) -> Self {
        Self { repo_id }
    }

    pub async fn check_repo_lock_with_transaction(
        &self,
        txn: Transaction,
    ) -> Result<(Transaction, RepoLockState), Error> {
        let (txn, row) = GetRepoLockStatus::query_with_transaction(txn, &self.repo_id)
            .await
            .context("Failed to query repo lock status")?;

        let state = row
            .first()
            .map_or(Ok(RepoLockState::Unlocked), convert_sql_state)?;

        Ok((txn, state))
    }
}

#[derive(Debug, Clone)]
pub struct MutableRepoLock {
    repo_id: RepositoryId,
    sql_repo_lock: SqlRepoLock,
}

impl MutableRepoLock {
    pub fn new(sql_repo_lock: SqlRepoLock, repo_id: RepositoryId) -> Self {
        Self {
            repo_id,
            sql_repo_lock,
        }
    }
}

#[async_trait]
impl RepoLock for MutableRepoLock {
    async fn check_repo_lock(&self, ctx: &CoreContext) -> Result<RepoLockState, Error> {
        let row = GetRepoLockStatus::query(
            &self.sql_repo_lock.read_connection,
            ctx.sql_query_telemetry(),
            &self.repo_id,
        )
        .await
        .context("Failed to query repo lock status")?;

        row.first()
            .map_or(Ok(RepoLockState::Unlocked), convert_sql_state)
    }

    async fn all_repos_lock(
        &self,
        ctx: &CoreContext,
    ) -> Result<HashMap<RepositoryId, RepoLockState>, Error> {
        let rows = AllReposLockStatus::query(
            &self.sql_repo_lock.read_connection,
            ctx.sql_query_telemetry(),
        )
        .await
        .context("Failed to query repo lock status")?;

        rows.into_iter()
            .map(|(repo_id, state, reason)| Ok((repo_id, convert_sql_state(&(state, reason))?)))
            .collect()
    }

    async fn set_repo_lock(
        &self,
        ctx: &CoreContext,
        lock_state: RepoLockState,
    ) -> Result<bool, Error> {
        let (state, reason) = match lock_state {
            RepoLockState::Unlocked => (0, None),
            RepoLockState::Locked(reason) => (1, Some(reason)),
        };

        SetRepoLockStatus::query(
            &self.sql_repo_lock.write_connection,
            ctx.sql_query_telemetry(),
            &self.repo_id,
            &state,
            &reason.as_deref(),
        )
        .await
        .map(|res| res.affected_rows() > 0)
    }
}

#[derive(Debug, Clone)]
pub struct AlwaysLockedRepoLock {
    repo_id: RepositoryId,
    reason: String,
}

impl AlwaysLockedRepoLock {
    pub fn new(repo_id: RepositoryId, reason: String) -> Self {
        Self { repo_id, reason }
    }
}

#[async_trait]
impl RepoLock for AlwaysLockedRepoLock {
    async fn check_repo_lock(&self, _ctx: &CoreContext) -> Result<RepoLockState, Error> {
        Ok(RepoLockState::Locked(self.reason.clone()))
    }

    async fn all_repos_lock(
        &self,
        _ctx: &CoreContext,
    ) -> Result<HashMap<RepositoryId, RepoLockState>, Error> {
        Ok(hashmap! { self.repo_id => RepoLockState::Locked(self.reason.clone()) })
    }

    async fn set_repo_lock(&self, _ctx: &CoreContext, _: RepoLockState) -> Result<bool, Error> {
        Err(anyhow!("Repo is locked in config and can't be updated"))
    }
}

#[derive(Debug, Clone)]
pub struct AlwaysUnlockedRepoLock {
    repo_id: RepositoryId,
}

impl AlwaysUnlockedRepoLock {
    pub fn new(repo_id: RepositoryId) -> Self {
        Self { repo_id }
    }
}

#[async_trait]
impl RepoLock for AlwaysUnlockedRepoLock {
    async fn check_repo_lock(&self, _ctx: &CoreContext) -> Result<RepoLockState, Error> {
        Ok(RepoLockState::Unlocked)
    }

    async fn all_repos_lock(
        &self,
        _ctx: &CoreContext,
    ) -> Result<HashMap<RepositoryId, RepoLockState>, Error> {
        Ok(hashmap! { self.repo_id => RepoLockState::Unlocked })
    }

    async fn set_repo_lock(&self, _ctx: &CoreContext, _: RepoLockState) -> Result<bool, Error> {
        Err(anyhow!("Repo is always unlocked and can't be updated"))
    }
}

#[cfg(test)]
mod test {
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;

    use super::*;

    mononoke_queries! {
        write InsertState(repo_id: RepositoryId, state: u8, reason: Option<&str>) {
            none,
            "INSERT OR REPLACE INTO repo_lock (repo_id, state, reason)
            VALUES ({repo_id}, {state}, {reason})"
        }
    }

    #[mononoke::fbinit_test]
    async fn test_locked(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let sql_repo_lock = SqlRepoLock::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(0);

        let repo_lock = MutableRepoLock::new(sql_repo_lock, repo_id);

        InsertState::query(
            &repo_lock.sql_repo_lock.clone().write_connection,
            ctx.sql_query_telemetry(),
            &repo_id,
            &1,
            &Some("reason"),
        )
        .await?;

        assert_eq!(
            repo_lock.check_repo_lock(&ctx).await?,
            RepoLockState::Locked("reason".to_string())
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_default_to_unlocked(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let sql_repo_lock = SqlRepoLock::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(0);

        let repo_lock = MutableRepoLock::new(sql_repo_lock, repo_id);

        assert_eq!(
            repo_lock.check_repo_lock(&ctx).await?,
            RepoLockState::Unlocked
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_lock_with_other_repo(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let sql_repo_lock = SqlRepoLock::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(0);
        let other_repo_id = RepositoryId::new(1);

        let repo_lock = MutableRepoLock::new(sql_repo_lock.clone(), repo_id);
        let other_repo_lock = MutableRepoLock::new(sql_repo_lock, other_repo_id);

        assert!(
            repo_lock
                .set_repo_lock(&ctx, RepoLockState::Locked("test".into()))
                .await?,
        );
        assert_eq!(
            repo_lock.check_repo_lock(&ctx).await?,
            RepoLockState::Locked("test".into())
        );
        assert_eq!(
            other_repo_lock.check_repo_lock(&ctx).await?,
            RepoLockState::Unlocked
        );

        Ok(())
    }
}
