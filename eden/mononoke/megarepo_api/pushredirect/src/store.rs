/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::_macro_internal::SqlQueryConfig;
use sql_ext::SqlConnections;
use sql_ext::mononoke_queries;

use crate::PushRedirectionConfig;
use crate::PushRedirectionConfigEntry;
use crate::RowId;

mononoke_queries! {
    read TestGet(id: RowId) -> (
        RowId,
        RepositoryId,
        bool,
        bool,
    ) {
        "SELECT id,
            repo_id,
            draft_push,
            public_push
         FROM pushredirect
         WHERE id = {id}"
    }

    cacheable read Get(repo_id: RepositoryId) -> (
        RowId,
        RepositoryId,
        bool,
        bool,
    ) {
        "SELECT id,
            repo_id,
            draft_push,
            public_push
         FROM pushredirect
         WHERE repo_id = {repo_id}"
    }

    write Set(repo_id: RepositoryId, draft_push: bool, public_push: bool) {
        none,
        mysql("INSERT INTO pushredirect (repo_id, draft_push, public_push) VALUES ({repo_id}, {draft_push}, {public_push}) ON DUPLICATE KEY UPDATE draft_push = {draft_push}, public_push = {public_push}")
        sqlite("REPLACE INTO pushredirect (repo_id, draft_push, public_push) VALUES ({repo_id}, {draft_push}, {public_push})")
    }
}

fn row_to_entry(row: (RowId, RepositoryId, bool, bool)) -> PushRedirectionConfigEntry {
    let (id, repo_id, draft_push, public_push) = row;
    PushRedirectionConfigEntry {
        id,
        repo_id,
        draft_push,
        public_push,
    }
}

pub struct SqlPushRedirectionConfig {
    connections: SqlConnections,
    sql_query_config: Arc<SqlQueryConfig>,
}

#[derive(Clone)]
pub struct SqlPushRedirectionConfigBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlPushRedirectionConfigBuilder {
    const LABEL: &'static str = "pushredirect";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-pushredirect.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlPushRedirectionConfigBuilder {
    pub fn build(self, sql_query_config: Arc<SqlQueryConfig>) -> SqlPushRedirectionConfig {
        let SqlPushRedirectionConfigBuilder { connections } = self;

        SqlPushRedirectionConfig {
            connections,
            sql_query_config,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlPushRedirectionConfigBuilder {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&RemoteDatabaseConfig> {
        Some(&remote.production)
    }
    fn oss_remote_database_config(
        remote: &OssRemoteMetadataDatabaseConfig,
    ) -> Option<&OssRemoteDatabaseConfig> {
        Some(&remote.production)
    }
}

#[async_trait]
impl PushRedirectionConfig for SqlPushRedirectionConfig {
    async fn set(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        draft_push: bool,
        public_push: bool,
    ) -> Result<()> {
        Set::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            &repo_id,
            &draft_push,
            &public_push,
        )
        .await?;
        Ok(())
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<Option<PushRedirectionConfigEntry>> {
        let ttl =
            justknobs::get_as::<u64>("scm/mononoke:pushredirection_config_cache_ttl_secs", None)?;

        let rows = Get::query(
            self.sql_query_config.as_ref(),
            if ttl == 0 {
                None
            } else {
                Some(std::time::Duration::from_secs(ttl))
            },
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            &repo_id,
        )
        .await?;
        Ok(rows.into_iter().next().map(row_to_entry))
    }
}

#[cfg(test)]
mod test {
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::fbinit_test]
    async fn test_set(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let builder = SqlPushRedirectionConfigBuilder::with_sqlite_in_memory()?;
        let sql_query_config = Arc::new(SqlQueryConfig { caching: None });
        let push = builder.clone().build(sql_query_config.clone());

        // insert one
        let repo_id = RepositoryId::new(1);
        push.set(&ctx, repo_id, true, false).await?;
        let entry = push.get(&ctx, repo_id).await?;
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert!(entry.draft_push);
        assert!(!entry.public_push);

        let push = builder.build(sql_query_config);

        // insert another
        let repo_id = RepositoryId::new(2);
        push.set(&ctx, repo_id, false, true).await?;
        let entry = push.get(&ctx, repo_id).await?;
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert!(!entry.draft_push);
        assert!(entry.public_push);

        // update it
        push.set(&ctx, repo_id, true, true).await?;
        let entry = push.get(&ctx, repo_id).await?;
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert!(entry.draft_push);
        assert!(entry.public_push);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_get(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let builder = SqlPushRedirectionConfigBuilder::with_sqlite_in_memory()?;
        let sql_query_config = Arc::new(SqlQueryConfig { caching: None });
        let push = builder.build(sql_query_config);

        let repo_id = RepositoryId::new(1);
        let entry = push.get(&ctx, repo_id).await?;
        assert!(entry.is_none());

        push.set(&ctx, repo_id, true, true).await?;
        let entry = push.get(&ctx, repo_id).await?;
        assert!(entry.is_some());
        let entry = entry.unwrap();
        assert!(entry.draft_push);
        assert!(entry.public_push);

        Ok(())
    }
}
