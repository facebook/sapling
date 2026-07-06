/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::DerivableType;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::Connection;
use sql_ext::SqlConnections;
use sql_ext::mononoke_queries;

use crate::EnabledDerivedDataTypeEntry;
use crate::EnabledDerivedDataTypes;
use crate::SqlDerivableType;
use crate::Staleness;

mononoke_queries! {
    read GetEnabledTypes(repo_id: RepositoryId) -> (
        RepositoryId,
        SqlDerivableType,
        Option<u64>,
    ) {
        "SELECT repo_id, derived_data_type, root_request_id
         FROM enabled_derived_data_types
         WHERE repo_id = {repo_id}"
    }

    read GetAll() -> (
        RepositoryId,
        SqlDerivableType,
        Option<u64>,
    ) {
        "SELECT repo_id, derived_data_type, root_request_id
         FROM enabled_derived_data_types"
    }

    // Idempotent upsert: never clobber an existing row (its root_request_id is
    // preserved). MySQL uses a no-op ON DUPLICATE KEY UPDATE; SQLite uses
    // INSERT OR IGNORE.
    write MarkEnabled(
        repo_id: RepositoryId,
        derived_data_type: SqlDerivableType,
        root_request_id: Option<u64>,
    ) {
        none,
        mysql("INSERT INTO enabled_derived_data_types (repo_id, derived_data_type, root_request_id) VALUES ({repo_id}, {derived_data_type}, {root_request_id}) ON DUPLICATE KEY UPDATE repo_id = repo_id")
        sqlite("INSERT OR IGNORE INTO enabled_derived_data_types (repo_id, derived_data_type, root_request_id) VALUES ({repo_id}, {derived_data_type}, {root_request_id})")
    }

    // Idempotent delete: removing an absent row affects zero rows and succeeds.
    write MarkDisabled(repo_id: RepositoryId, derived_data_type: SqlDerivableType) {
        none,
        "DELETE FROM enabled_derived_data_types
         WHERE repo_id = {repo_id} AND derived_data_type = {derived_data_type}"
    }
}

fn row_to_entry(row: (RepositoryId, SqlDerivableType, Option<u64>)) -> EnabledDerivedDataTypeEntry {
    let (repo_id, derived_data_type, root_request_id) = row;
    EnabledDerivedDataTypeEntry {
        repo_id,
        derived_data_type: derived_data_type.0,
        root_request_id,
    }
}

pub struct SqlEnabledDerivedDataTypes {
    connections: SqlConnections,
}

impl SqlEnabledDerivedDataTypes {
    fn get_connection(&self, staleness: Staleness) -> &Connection {
        match staleness {
            Staleness::MostRecent => &self.connections.read_master_connection,
            Staleness::MaybeStale => &self.connections.read_connection,
        }
    }
}

#[derive(Clone)]
pub struct SqlEnabledDerivedDataTypesBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlEnabledDerivedDataTypesBuilder {
    const LABEL: &'static str = "enabled_derived_data_types";

    const CREATION_QUERY: &'static str =
        include_str!("../schemas/sqlite-enabled-derived-data-types.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlEnabledDerivedDataTypesBuilder {
    pub fn build(self) -> SqlEnabledDerivedDataTypes {
        let SqlEnabledDerivedDataTypesBuilder { connections } = self;
        SqlEnabledDerivedDataTypes { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlEnabledDerivedDataTypesBuilder {
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
impl EnabledDerivedDataTypes for SqlEnabledDerivedDataTypes {
    async fn mark_enabled(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        derived_data_type: DerivableType,
        root_request_id: Option<u64>,
    ) -> Result<()> {
        MarkEnabled::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            &repo_id,
            &SqlDerivableType(derived_data_type),
            &root_request_id,
        )
        .await?;
        Ok(())
    }

    async fn mark_disabled(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        derived_data_type: DerivableType,
    ) -> Result<()> {
        MarkDisabled::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            &repo_id,
            &SqlDerivableType(derived_data_type),
        )
        .await?;
        Ok(())
    }

    async fn get_enabled_types(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        staleness: Staleness,
    ) -> Result<Vec<DerivableType>> {
        let rows = GetEnabledTypes::query(
            self.get_connection(staleness),
            ctx.sql_query_telemetry(),
            &repo_id,
        )
        .await?;
        Ok(rows
            .into_iter()
            .map(|(_, derived_data_type, _)| derived_data_type.0)
            .collect())
    }

    async fn get_all(&self, ctx: &CoreContext) -> Result<Vec<EnabledDerivedDataTypeEntry>> {
        let rows = GetAll::query(
            &self.connections.read_master_connection,
            ctx.sql_query_telemetry(),
        )
        .await?;
        Ok(rows.into_iter().map(row_to_entry).collect())
    }
}

#[cfg(all(fbcode_build, test))]
mod test {
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::fbinit_test]
    async fn test_mark_enabled_then_get(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let store = SqlEnabledDerivedDataTypesBuilder::with_sqlite_in_memory()?.build();

        let repo_id = RepositoryId::new(1);
        let ddt = DerivableType::GitDeltaManifestsV3;

        assert!(
            store
                .get_enabled_types(&ctx, repo_id, Staleness::MostRecent)
                .await?
                .is_empty(),
            "no types should be enabled before mark_enabled"
        );

        store.mark_enabled(&ctx, repo_id, ddt, Some(42)).await?;

        let enabled = store
            .get_enabled_types(&ctx, repo_id, Staleness::MostRecent)
            .await?;
        assert_eq!(enabled, vec![ddt], "the marked type should be returned");

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_mark_enabled_is_idempotent(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let store = SqlEnabledDerivedDataTypesBuilder::with_sqlite_in_memory()?.build();

        let repo_id = RepositoryId::new(1);
        let ddt = DerivableType::GitDeltaManifestsV3;

        // First mark records root_request_id 42.
        store.mark_enabled(&ctx, repo_id, ddt, Some(42)).await?;
        // Second mark with a different root_request_id must be a no-op, not a clobber.
        store.mark_enabled(&ctx, repo_id, ddt, Some(99)).await?;

        let all = store.get_all(&ctx).await?;
        assert_eq!(all.len(), 1, "re-marking must not create a second row");
        assert_eq!(
            all[0].root_request_id,
            Some(42),
            "the original root_request_id must be preserved on idempotent re-mark"
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_mark_disabled(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let store = SqlEnabledDerivedDataTypesBuilder::with_sqlite_in_memory()?.build();

        let repo_id = RepositoryId::new(1);
        let ddt = DerivableType::GitDeltaManifestsV3;
        let other = DerivableType::Unodes;

        // Disabling an absent row is a no-op success.
        store.mark_disabled(&ctx, repo_id, ddt).await?;

        store.mark_enabled(&ctx, repo_id, ddt, Some(42)).await?;
        store.mark_enabled(&ctx, repo_id, other, None).await?;

        store.mark_disabled(&ctx, repo_id, ddt).await?;

        let enabled = store
            .get_enabled_types(&ctx, repo_id, Staleness::MostRecent)
            .await?;
        assert_eq!(
            enabled,
            vec![other],
            "only the disabled type should be removed; the other remains"
        );

        // Disabling the same row again is still a no-op success.
        store.mark_disabled(&ctx, repo_id, ddt).await?;

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_get_all_cross_repo(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let store = SqlEnabledDerivedDataTypesBuilder::with_sqlite_in_memory()?.build();

        store
            .mark_enabled(
                &ctx,
                RepositoryId::new(1),
                DerivableType::GitDeltaManifestsV3,
                Some(1),
            )
            .await?;
        store
            .mark_enabled(&ctx, RepositoryId::new(1), DerivableType::Unodes, None)
            .await?;
        store
            .mark_enabled(
                &ctx,
                RepositoryId::new(2),
                DerivableType::GitDeltaManifestsV3,
                Some(2),
            )
            .await?;

        let all = store.get_all(&ctx).await?;
        assert_eq!(all.len(), 3, "get_all should return rows across all repos");

        let repo2 = store
            .get_enabled_types(&ctx, RepositoryId::new(2), Staleness::MostRecent)
            .await?;
        assert_eq!(
            repo2,
            vec![DerivableType::GitDeltaManifestsV3],
            "repo 2 has exactly one enabled type"
        );

        Ok(())
    }
}
