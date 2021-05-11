/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */


#![deny(warnings)]
use anyhow::{anyhow, Error};
use context::{CoreContext, PerfCounterType};
pub use megarepo_configs::types::{
    Source, SourceMappingRules, SourceRevision, SyncConfigVersion, SyncTargetConfig, Target,
};
use mononoke_types::ChangesetId;
use sql::{queries, Connection, Transaction};
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::SqlConnections;

queries! {
    read GetLatestSyncedSourceChangeset(
        source_name: String,
        target_repo_id: i64,
        target_bookmark: String,
    ) -> (ChangesetId) {
        "SELECT source_bcs_id
          FROM megarepo_latest_synced_source_changeset
          WHERE source_name = {source_name}
          AND target_repo_id = {target_repo_id}
          AND target_bookmark = {target_bookmark}
          "
    }

    read GetTargetConfigVersion(
        target_repo_id: i64,
        target_bookmark: String,
        target_bcs_id: ChangesetId,
    ) -> (SyncConfigVersion) {
        "SELECT sync_config_version
          FROM megarepo_changeset_mapping
          WHERE target_repo_id = {target_repo_id}
          AND target_bookmark = {target_bookmark}
          AND target_bcs_id = {target_bcs_id}
          "
    }

    write UpdateLatestSyncedCommitFromSource(values: (
        source_name: String,
        target_repo_id: i64,
        target_bookmark: String,
        source_bcs_id: ChangesetId,
    )) {
        none,
        mysql("INSERT INTO megarepo_latest_synced_source_changeset
        (source_name, target_repo_id, target_bookmark, source_bcs_id)
        VALUES {values}
        ON DUPLICATE KEY UPDATE source_bcs_id = VALUES(source_bcs_id)")
        sqlite("INSERT OR REPLACE INTO megarepo_latest_synced_source_changeset
        (source_name, target_repo_id, target_bookmark, source_bcs_id)
        VALUES {values}")
    }

    write InsertMapping(values: (
        source_name: String,
        target_repo_id: i64,
        target_bookmark: String,
        source_bcs_id: ChangesetId,
        target_bcs_id: ChangesetId,
        sync_config_version: SyncConfigVersion,
    )) {
        none,
        "INSERT INTO megarepo_changeset_mapping
        (source_name, target_repo_id, target_bookmark, source_bcs_id, target_bcs_id, sync_config_version)
        VALUES {values}"
    }
}

pub struct MegarepoMapping {
    pub(crate) connections: SqlConnections,
}

impl MegarepoMapping {
    /// For a given (source, target) pair return the latest changeset that was
    /// synced from a source into a given target.
    ///
    /// This method can be used for validation - we can check the new commit that's synced via
    /// sync_changeset() is an ancestor of the latest synced changeset
    #[allow(clippy::ptr_arg)]
    pub async fn get_latest_synced_commit_from_source(
        &self,
        ctx: &CoreContext,
        source_name: &String,
        target: &Target,
    ) -> Result<Option<ChangesetId>, Error> {
        let maybe_cs_id = self
            .get_latest_synced_commit_from_source_impl(
                ctx,
                source_name,
                target,
                PerfCounterType::SqlReadsReplica,
                &self.connections.read_connection,
            )
            .await?;

        if let Some(cs_id) = maybe_cs_id {
            return Ok(Some(cs_id));
        }

        self.get_latest_synced_commit_from_source_impl(
            ctx,
            source_name,
            target,
            PerfCounterType::SqlReadsMaster,
            &self.connections.read_master_connection,
        )
        .await
    }

    #[allow(clippy::ptr_arg)]
    async fn get_latest_synced_commit_from_source_impl(
        &self,
        ctx: &CoreContext,
        source_name: &String,
        target: &Target,
        sql_perf_counter: PerfCounterType,
        connection: &Connection,
    ) -> Result<Option<ChangesetId>, Error> {
        ctx.perf_counters().increment_counter(sql_perf_counter);
        let mut rows = GetLatestSyncedSourceChangeset::query(
            &connection,
            source_name,
            &target.repo_id,
            &target.bookmark,
        )
        .await?;

        if rows.len() > 1 {
            return Err(anyhow!(
                "Programming error - more than 1 row returned for latest synced commit"
            ));
        }

        Ok(rows.pop().map(|x| x.0))
    }

    /// For a given (target, cs_id) pair return the version that was used
    /// to create target changeset id.
    /// Usually this method is used to find what version do we need to use
    /// for rewriting a commit
    pub async fn get_target_config_version(
        &self,
        ctx: &CoreContext,
        target: &Target,
        target_cs_id: ChangesetId,
    ) -> Result<Option<SyncConfigVersion>, Error> {
        let maybe_version = self
            .get_target_config_version_impl(
                ctx,
                target,
                target_cs_id,
                PerfCounterType::SqlReadsReplica,
                &self.connections.read_connection,
            )
            .await?;

        if let Some(version) = maybe_version {
            return Ok(Some(version));
        }

        self.get_target_config_version_impl(
            ctx,
            target,
            target_cs_id,
            PerfCounterType::SqlReadsMaster,
            &self.connections.read_master_connection,
        )
        .await
    }

    async fn get_target_config_version_impl(
        &self,
        ctx: &CoreContext,
        target: &Target,
        target_cs_id: ChangesetId,
        sql_perf_counter: PerfCounterType,
        connection: &Connection,
    ) -> Result<Option<SyncConfigVersion>, Error> {
        ctx.perf_counters().increment_counter(sql_perf_counter);
        let mut rows = GetTargetConfigVersion::query(
            &connection,
            &target.repo_id,
            &target.bookmark,
            &target_cs_id,
        )
        .await?;

        if rows.len() > 1 {
            return Err(anyhow!(
                "Programming error - more than 1 row returned for get target config version"
            ));
        }

        Ok(rows.pop().map(|x| x.0))
    }

    /// Update the latest synced commit from a source in a given transaction.
    /// The transaction is usually a bookmark move, so that we updated latest synced
    /// commit only if a bookmark move in a target repo was successful.
    #[allow(clippy::ptr_arg)]
    pub async fn update_latest_synced_commit_from_source(
        &self,
        ctx: &CoreContext,
        txn: Transaction,
        source_name: &String,
        target: &Target,
        source_cs_id: ChangesetId,
    ) -> Result<Transaction, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        let (txn, _) = UpdateLatestSyncedCommitFromSource::query_with_transaction(
            txn,
            &[(
                source_name,
                &target.repo_id,
                &target.bookmark,
                &source_cs_id,
            )],
        )
        .await?;

        Ok(txn)
    }

    /// Add a mapping from a source commit to a target commit
    #[allow(clippy::ptr_arg)]
    pub async fn insert_source_target_cs_mapping(
        &self,
        ctx: &CoreContext,
        source_name: &String,
        target: &Target,
        source_cs_id: ChangesetId,
        target_cs_id: ChangesetId,
        version: &SyncConfigVersion,
    ) -> Result<(), Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        InsertMapping::query(
            &self.connections.write_connection,
            &[(
                source_name,
                &target.repo_id,
                &target.bookmark,
                &source_cs_id,
                &target_cs_id,
                &version,
            )],
        )
        .await?;

        Ok(())
    }
}

impl SqlConstruct for MegarepoMapping {
    const LABEL: &'static str = "megarepo_mapping";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-megarepo-mapping.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for MegarepoMapping {}

#[cfg(test)]
mod test {
    use super::*;
    use fbinit::FacebookInit;
    use mononoke_types_mocks::changesetid::{ONES_CSID, TWOS_CSID};

    #[fbinit::test]
    async fn test_simple_mapping(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mapping = MegarepoMapping::with_sqlite_in_memory()?;

        let target = Target {
            repo_id: 0,
            bookmark: "book".to_string(),
        };

        let source_csid = ONES_CSID;
        let target_csid = TWOS_CSID;
        let version = "version".to_string();

        mapping
            .insert_source_target_cs_mapping(
                &ctx,
                &"source_name".to_string(),
                &target,
                source_csid,
                target_csid,
                &version,
            )
            .await?;

        let res = mapping
            .get_target_config_version(&ctx, &target, target_csid)
            .await?;

        assert_eq!(res, Some(version));

        Ok(())
    }

    #[fbinit::test]
    async fn test_simple_latest_synced_changeset(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mapping = MegarepoMapping::with_sqlite_in_memory()?;

        let txn = mapping
            .connections
            .write_connection
            .start_transaction()
            .await?;

        let target = Target {
            repo_id: 0,
            bookmark: "book".to_string(),
        };

        let source_name = "source_name".to_string();
        let source_csid = ONES_CSID;

        let txn = mapping
            .update_latest_synced_commit_from_source(&ctx, txn, &source_name, &target, source_csid)
            .await?;

        txn.commit().await?;

        let cs_id = mapping
            .get_latest_synced_commit_from_source(&ctx, &source_name, &target)
            .await?;

        assert_eq!(Some(source_csid), cs_id);

        Ok(())
    }
}
