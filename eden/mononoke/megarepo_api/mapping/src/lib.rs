/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */


#![deny(warnings)]
use anyhow::{anyhow, Context, Error};
use blobrepo::BlobRepo;
use context::{CoreContext, PerfCounterType};
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use manifest::{Entry, ManifestOps};
pub use megarepo_configs::types::{
    Source, SourceMappingRules, SourceRevision, SyncConfigVersion, SyncTargetConfig, Target,
};
use mononoke_types::{BonsaiChangesetMut, ChangesetId, ContentId, FileChange, FileType, MPath};
use serde::{Deserialize, Serialize};
use sql::{queries, Connection};
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::SqlConnections;
use std::collections::BTreeMap;

queries! {
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

pub const REMAPPING_STATE_FILE: &str = ".megarepo/remapping_state";

#[derive(Clone, Serialize, Deserialize)]
pub struct CommitRemappingState {
    /// Mapping from source to a changeset id
    latest_synced_changesets: BTreeMap<String, ChangesetId>,
    /// Config version that was used to create this commit
    sync_config_version: SyncConfigVersion,
}

impl CommitRemappingState {
    pub fn new(
        latest_synced_changesets: BTreeMap<String, ChangesetId>,
        sync_config_version: SyncConfigVersion,
    ) -> Self {
        Self {
            latest_synced_changesets,
            sync_config_version,
        }
    }

    pub async fn read_state_from_commit(
        ctx: &CoreContext,
        repo: &BlobRepo,
        cs_id: ChangesetId,
    ) -> Result<Self, Error> {
        let root_fsnode_id = RootFsnodeId::derive(ctx, repo, cs_id).await?;

        let path = MPath::new(REMAPPING_STATE_FILE)?;
        let maybe_entry = root_fsnode_id
            .fsnode_id()
            .find_entry(ctx.clone(), repo.get_blobstore(), Some(path))
            .await?;

        let entry = maybe_entry.ok_or_else(|| anyhow!("{} not found", REMAPPING_STATE_FILE))?;

        let file = match entry {
            Entry::Tree(_) => {
                return Err(anyhow!(
                    "{} is a directory, but should be a file!",
                    REMAPPING_STATE_FILE
                ));
            }
            Entry::Leaf(file) => file,
        };

        let bytes = filestore::fetch_concat(repo.blobstore(), ctx, *file.content_id()).await?;
        let content = String::from_utf8(bytes.to_vec())
            .with_context(|| format!("{} is not utf8", REMAPPING_STATE_FILE))?;
        let state: CommitRemappingState = serde_json::from_str(&content)?;
        Ok(state)
    }

    pub async fn save_in_changeset(
        &self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        bcs: &mut BonsaiChangesetMut,
    ) -> Result<(), Error> {
        let (content_id, size) = self.save(ctx, repo).await?;
        let path = MPath::new(REMAPPING_STATE_FILE)?;

        let fc = FileChange::new(content_id, FileType::Regular, size, None);
        if bcs.file_changes.insert(path, Some(fc)).is_some() {
            return Err(anyhow!("New bonsai changeset already has {} file"));
        }

        Ok(())
    }

    pub fn set_source_changeset(&mut self, source: &str, cs_id: ChangesetId) {
        self.latest_synced_changesets
            .insert(source.to_string(), cs_id);
    }

    pub fn get_latest_synced_changeset(&self, source: &str) -> Option<&ChangesetId> {
        self.latest_synced_changesets.get(source)
    }

    pub fn sync_config_version(&self) -> &SyncConfigVersion {
        &self.sync_config_version
    }

    async fn save(&self, ctx: &CoreContext, repo: &BlobRepo) -> Result<(ContentId, u64), Error> {
        let bytes = self.serialize()?;

        let ((content_id, size), fut) =
            filestore::store_bytes(repo.blobstore(), repo.filestore_config(), ctx, bytes.into());

        fut.await?;

        Ok((content_id, size))
    }

    fn serialize(&self) -> Result<Vec<u8>, Error> {
        serde_json::to_vec_pretty(&self).map_err(Error::from)
    }
}

impl MegarepoMapping {
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
    use maplit::btreemap;
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
    async fn test_serialize(_fb: FacebookInit) -> Result<(), Error> {
        let state = CommitRemappingState::new(
            btreemap! {
                "source_1".to_string() => ONES_CSID,
                "source_2".to_string() => TWOS_CSID,
            },
            "version_1".to_string(),
        );
        let res = String::from_utf8(state.serialize()?)?;
        assert_eq!(
            res,
            r#"{
  "latest_synced_changesets": {
    "source_1": "1111111111111111111111111111111111111111111111111111111111111111",
    "source_2": "2222222222222222222222222222222222222222222222222222222222222222"
  },
  "sync_config_version": "version_1"
}"#
        );

        Ok(())
    }
}
