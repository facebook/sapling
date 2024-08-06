/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::RepositoryId;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::mononoke_queries;
use sql_ext::SqlConnections;

use crate::GitPushRedirectConfig;
use crate::GitPushRedirectConfigEntry;
use crate::RowId;
use crate::Staleness;

mononoke_queries! {
    read TestGet(id: RowId) -> (
        RowId,
        RepositoryId,
        bool,
    ) {
        "SELECT id,
            repo_id,
            mononoke
         FROM git_push_redirect
         WHERE id = {id}"
    }

    read GetByRepoId(repo_id: RepositoryId) -> (
        RowId,
        RepositoryId,
        bool,
    ) {
        "SELECT id,
            repo_id,
            mononoke
         FROM git_push_redirect
         WHERE repo_id = {repo_id}"
    }

    read GetByGitPushRedirect(mononoke: bool) -> (
        RowId,
        RepositoryId,
        bool,
    ) {
        "SELECT id,
            repo_id,
            mononoke
         FROM git_push_redirect
         WHERE mononoke = {mononoke}"
    }

    write Set(repo_id: RepositoryId, mononoke: bool) {
        none,
        mysql("INSERT INTO git_push_redirect (repo_id, mononoke) VALUES ({repo_id}, {mononoke}) ON DUPLICATE KEY UPDATE mononoke = {mononoke}")
        sqlite("REPLACE INTO git_push_redirect (repo_id, mononoke) VALUES ({repo_id}, {mononoke})")
    }
}

fn row_to_entry(row: (RowId, RepositoryId, bool)) -> GitPushRedirectConfigEntry {
    let (id, repo_id, mononoke) = row;
    GitPushRedirectConfigEntry {
        id,
        repo_id,
        mononoke,
    }
}

pub struct SqlGitPushRedirectConfig {
    connections: SqlConnections,
}

impl SqlGitPushRedirectConfig {
    pub fn get_connection(&self, staleness: Staleness) -> &Connection {
        match staleness {
            Staleness::MostRecent => &self.connections.read_master_connection,
            Staleness::MaybeStale => &self.connections.read_connection,
        }
    }
}

#[derive(Clone)]
pub struct SqlGitPushRedirectConfigBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlGitPushRedirectConfigBuilder {
    const LABEL: &'static str = "git_push_redirect";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-git-push-redirect.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlGitPushRedirectConfigBuilder {
    pub fn build(self) -> SqlGitPushRedirectConfig {
        let SqlGitPushRedirectConfigBuilder { connections } = self;

        SqlGitPushRedirectConfig { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlGitPushRedirectConfigBuilder {}

#[async_trait]
impl GitPushRedirectConfig for SqlGitPushRedirectConfig {
    async fn set(&self, _ctx: &CoreContext, repo_id: RepositoryId, mononoke: bool) -> Result<()> {
        Set::query(&self.connections.write_connection, &repo_id, &mononoke).await?;
        Ok(())
    }

    async fn get_by_repo_id(
        &self,
        _ctx: &CoreContext,
        repo_id: RepositoryId,
        staleness: Staleness,
    ) -> Result<Option<GitPushRedirectConfigEntry>> {
        let rows = GetByRepoId::query(self.get_connection(staleness), &repo_id).await?;
        Ok(rows.into_iter().next().map(row_to_entry))
    }

    async fn get_redirected_to_mononoke(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitPushRedirectConfigEntry>> {
        let rows =
            GetByGitPushRedirect::query(&self.connections.read_master_connection, &true).await?;
        Ok(rows.into_iter().map(row_to_entry).collect())
    }

    async fn get_redirected_to_metagit(
        &self,
        _ctx: &CoreContext,
    ) -> Result<Vec<GitPushRedirectConfigEntry>> {
        let rows =
            GetByGitPushRedirect::query(&self.connections.read_master_connection, &false).await?;
        Ok(rows.into_iter().map(row_to_entry).collect())
    }
}

#[cfg(test)]
mod test {
    use fbinit::FacebookInit;

    use super::*;

    #[fbinit::test]
    async fn test_set(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let builder = SqlGitPushRedirectConfigBuilder::with_sqlite_in_memory()?;
        let push = builder.clone().build();

        // insert one
        let repo_id = RepositoryId::new(1);
        push.set(&ctx, repo_id, true).await?;
        let entry = push
            .get_by_repo_id(&ctx, repo_id, Staleness::MostRecent)
            .await?;
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert!(entry.mononoke);

        let push = builder.build();

        // insert another
        let repo_id = RepositoryId::new(2);
        push.set(&ctx, repo_id, false).await?;
        let entry = push
            .get_by_repo_id(&ctx, repo_id, Staleness::MostRecent)
            .await?;
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert!(!entry.mononoke);

        // update it
        push.set(&ctx, repo_id, true).await?;
        let entry = push
            .get_by_repo_id(&ctx, repo_id, Staleness::MostRecent)
            .await?;
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert!(entry.mononoke);

        Ok(())
    }

    #[fbinit::test]
    async fn test_get_by_repo_id(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let builder = SqlGitPushRedirectConfigBuilder::with_sqlite_in_memory()?;
        let push = builder.build();

        let repo_id = RepositoryId::new(1);
        let entry = push
            .get_by_repo_id(&ctx, repo_id, Staleness::MostRecent)
            .await?;
        assert!(entry.is_none());

        push.set(&ctx, repo_id, true).await?;
        let entry = push
            .get_by_repo_id(&ctx, repo_id, Staleness::MostRecent)
            .await?;
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert!(entry.mononoke);

        Ok(())
    }

    #[fbinit::test]
    async fn test_get_redirected(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let builder = SqlGitPushRedirectConfigBuilder::with_sqlite_in_memory()?;
        let push = builder.build();

        let to_be_redirected_to_mononoke_repo_id = RepositoryId::new(1);
        push.set(&ctx, to_be_redirected_to_mononoke_repo_id, false)
            .await?;
        push.set(&ctx, RepositoryId::new(2), true).await?;
        push.set(&ctx, RepositoryId::new(3), false).await?;

        assert_eq!(push.get_redirected_to_mononoke(&ctx).await?.len(), 1);
        assert_eq!(push.get_redirected_to_metagit(&ctx).await?.len(), 2);

        push.set(&ctx, to_be_redirected_to_mononoke_repo_id, true)
            .await?;

        assert_eq!(push.get_redirected_to_mononoke(&ctx).await?.len(), 2);
        assert_eq!(push.get_redirected_to_metagit(&ctx).await?.len(), 1);

        Ok(())
    }
}
