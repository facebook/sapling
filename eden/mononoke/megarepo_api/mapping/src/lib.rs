/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

use std::collections::BTreeMap;
use std::fmt;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use content_manifest_derivation::RootContentManifestId;
use context::CoreContext;
use context::PerfCounterType;
use filestore::FilestoreConfigRef;
use fsnodes::RootFsnodeId;
use manifest::Entry;
use manifest::ManifestOps;
pub use megarepo_configs::Source;
pub use megarepo_configs::SourceMappingRules;
pub use megarepo_configs::SourceRevision;
pub use megarepo_configs::SyncConfigVersion;
pub use megarepo_configs::SyncTargetConfig;
pub use megarepo_configs::Target;
use mononoke_types::content_manifest::compat::ContentManifestFile;
use mononoke_types::content_manifest::compat::ContentManifestId;
use mononoke_types::path::MPath;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::GitLfs;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;
use serde::Deserialize;
use serde::Serialize;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::mononoke_queries;
use sql_ext::SqlConnections;

pub trait Repo =
    RepoDerivedDataRef + RepoBlobstoreRef + FilestoreConfigRef + RepoIdentityRef + Sync + Send;

mononoke_queries! {
    read GetMappingEntry(
        target_repo_id: i64,
        target_bookmark: String,
        target_bcs_id: ChangesetId,
    ) -> (String, ChangesetId, SyncConfigVersion) {
        "SELECT source_name, source_bcs_id, sync_config_version
          FROM megarepo_changeset_mapping
          WHERE target_repo_id = {target_repo_id}
          AND target_bookmark = {target_bookmark}
          AND target_bcs_id = {target_bcs_id}
          "
    }

    write InsertMapping(values: (
        source_name: str,
        target_repo_id: i64,
        target_bookmark: String,
        source_bcs_id: ChangesetId,
        target_bcs_id: ChangesetId,
        sync_config_version: SyncConfigVersion,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO megarepo_changeset_mapping
        (source_name, target_repo_id, target_bookmark, source_bcs_id, target_bcs_id, sync_config_version)
        VALUES {values}"
    }

    read GetReverseMappingEntry(
        target_repo_id: i64,
        target_bookmark: String,
        source_bcs_id: ChangesetId,
    ) -> (String, ChangesetId, SyncConfigVersion) {
        "SELECT source_name, target_bcs_id, sync_config_version
        FROM megarepo_changeset_mapping
        WHERE target_repo_id = {target_repo_id}
        AND target_bookmark = {target_bookmark}
        AND source_bcs_id = {source_bcs_id}"
    }
}

pub struct MegarepoMapping {
    pub(crate) connections: SqlConnections,
}

pub const REMAPPING_STATE_FILE: &str = ".megarepo/remapping_state";

#[derive(
    Clone,
    Debug,
    Hash,
    Eq,
    Ord,
    PartialOrd,
    PartialEq,
    Serialize,
    Deserialize
)]
#[serde(transparent)]
pub struct SourceName(pub String);
impl SourceName {
    pub fn new<T: ToString>(name: T) -> Self {
        SourceName(name.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for SourceName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct CommitRemappingState {
    /// Mapping from source to a changeset id
    pub latest_synced_changesets: BTreeMap<SourceName, ChangesetId>,
    /// Config version that was used to create this commit
    sync_config_version: SyncConfigVersion,
    /// AOSP manifest branch that this branch is based on
    #[serde(skip_serializing_if = "Option::is_none")]
    bookmark: Option<String>,
}

impl CommitRemappingState {
    pub fn new(
        latest_synced_changesets: BTreeMap<SourceName, ChangesetId>,
        sync_config_version: SyncConfigVersion,
        bookmark: Option<String>,
    ) -> Self {
        Self {
            latest_synced_changesets,
            sync_config_version,
            bookmark,
        }
    }

    pub async fn read_state_from_commit(
        ctx: &CoreContext,
        repo: &impl Repo,
        cs_id: ChangesetId,
    ) -> Result<Self, Error> {
        let maybe_state = Self::read_state_from_commit_opt(ctx, repo, cs_id).await?;

        maybe_state.ok_or_else(|| anyhow!("file {} not found", REMAPPING_STATE_FILE))
    }

    pub async fn read_state_from_commit_opt(
        ctx: &CoreContext,
        repo: &impl Repo,
        cs_id: ChangesetId,
    ) -> Result<Option<Self>, Error> {
        let path = MPath::new(REMAPPING_STATE_FILE)?;

        let root_id: ContentManifestId = if let Ok(true) = justknobs::eval(
            "scm/mononoke:derived_data_use_content_manifests",
            None,
            Some(repo.repo_identity().name()),
        ) {
            repo.repo_derived_data()
                .derive::<RootContentManifestId>(ctx, cs_id)
                .await?
                .into_content_manifest_id()
                .into()
        } else {
            repo.repo_derived_data()
                .derive::<RootFsnodeId>(ctx, cs_id)
                .await?
                .into_fsnode_id()
                .into()
        };

        let entry = root_id
            .find_entry(ctx.clone(), repo.repo_blobstore().clone(), path)
            .await?;
        let content_id = match entry {
            Some(Entry::Leaf(file)) => ContentManifestFile(file).content_id(),
            _ => return Ok(None),
        };

        let bytes = filestore::fetch_concat(repo.repo_blobstore(), ctx, content_id).await?;
        let content = String::from_utf8(bytes.to_vec())
            .with_context(|| format!("{} is not utf8", REMAPPING_STATE_FILE))?;
        let state: CommitRemappingState = serde_json::from_str(&content)?;
        Ok(Some(state))
    }

    pub async fn save_in_changeset(
        &self,
        ctx: &CoreContext,
        repo: &impl Repo,
        bcs: &mut BonsaiChangesetMut,
    ) -> Result<(), Error> {
        let (content_id, size) = self.save(ctx, repo).await?;
        let path = NonRootMPath::new(REMAPPING_STATE_FILE)?;

        let fc = FileChange::tracked(
            content_id,
            FileType::Regular,
            size,
            None,
            GitLfs::FullContent,
        );
        if bcs.file_changes.insert(path, fc).is_some() {
            return Err(anyhow!(
                "New bonsai changeset already has {} file",
                REMAPPING_STATE_FILE,
            ));
        }

        Ok(())
    }

    pub fn set_bookmark(&mut self, bookmark: String) {
        self.bookmark = Some(bookmark);
    }

    pub fn set_source_changeset(&mut self, source: SourceName, cs_id: ChangesetId) {
        self.latest_synced_changesets.insert(source, cs_id);
    }

    pub fn get_latest_synced_changeset(&self, source: &SourceName) -> Option<&ChangesetId> {
        self.latest_synced_changesets.get(source)
    }

    pub fn get_all_latest_synced_changesets(&self) -> &BTreeMap<SourceName, ChangesetId> {
        &self.latest_synced_changesets
    }

    pub fn sync_config_version(&self) -> &SyncConfigVersion {
        &self.sync_config_version
    }

    async fn save(&self, ctx: &CoreContext, repo: &impl Repo) -> Result<(ContentId, u64), Error> {
        let bytes = self.serialize()?;

        let ((content_id, size), fut) = filestore::store_bytes(
            repo.repo_blobstore(),
            *repo.filestore_config(),
            ctx,
            bytes.into(),
        );

        fut.await?;

        Ok((content_id, size))
    }

    fn serialize(&self) -> Result<Vec<u8>, Error> {
        serde_json::to_vec_pretty(&self).map_err(Error::from)
    }
}

pub struct MegarepoMappingEntry {
    pub source_name: SourceName,
    pub target: Target,
    pub source_cs_id: ChangesetId,
    pub target_cs_id: ChangesetId,
    pub sync_config_version: SyncConfigVersion,
}

impl MegarepoMapping {
    /// For a given (target, cs_id) pair return the version that was used
    /// to create target changeset id.
    /// Usually this method is used to find what version do we need to use
    /// for rewriting a commit
    pub async fn get_mapping_entry(
        &self,
        ctx: &CoreContext,
        target: &Target,
        target_cs_id: ChangesetId,
    ) -> Result<Option<MegarepoMappingEntry>, Error> {
        let maybe_version = self
            .get_mapping_entry_impl(
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

        self.get_mapping_entry_impl(
            ctx,
            target,
            target_cs_id,
            PerfCounterType::SqlReadsMaster,
            &self.connections.read_master_connection,
        )
        .await
    }

    async fn get_mapping_entry_impl(
        &self,
        ctx: &CoreContext,
        target: &Target,
        target_cs_id: ChangesetId,
        sql_perf_counter: PerfCounterType,
        connection: &Connection,
    ) -> Result<Option<MegarepoMappingEntry>, Error> {
        ctx.perf_counters().increment_counter(sql_perf_counter);
        let mut rows = GetMappingEntry::maybe_traced_query(
            connection,
            ctx.client_request_info(),
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

        Ok(rows.pop().map(|x| MegarepoMappingEntry {
            source_name: SourceName::new(x.0),
            source_cs_id: x.1,
            sync_config_version: x.2,
            target: target.clone(),
            target_cs_id: target_cs_id.clone(),
        }))
    }

    // Reverse lookup of the previous query
    // It is possible to have same source_cs_id mapped to the different targets
    // but this method may return stale(not complete) set of mappings.
    pub async fn get_reverse_mapping_entry(
        &self,
        ctx: &CoreContext,
        target: &Target,
        source_cs_id: ChangesetId,
    ) -> Result<Vec<MegarepoMappingEntry>, Error> {
        let entries = self
            .get_reverse_mapping_entry_impl(
                ctx,
                target,
                source_cs_id,
                PerfCounterType::SqlReadsReplica,
                &self.connections.read_connection,
            )
            .await?;

        if !entries.is_empty() {
            return Ok(entries);
        }

        self.get_reverse_mapping_entry_impl(
            ctx,
            target,
            source_cs_id,
            PerfCounterType::SqlReadsMaster,
            &self.connections.read_master_connection,
        )
        .await
    }

    async fn get_reverse_mapping_entry_impl(
        &self,
        ctx: &CoreContext,
        target: &Target,
        source_cs_id: ChangesetId,
        sql_perf_counter: PerfCounterType,
        connection: &Connection,
    ) -> Result<Vec<MegarepoMappingEntry>, Error> {
        ctx.perf_counters().increment_counter(sql_perf_counter);
        let rows = GetReverseMappingEntry::maybe_traced_query(
            connection,
            ctx.client_request_info(),
            &target.repo_id,
            &target.bookmark,
            &source_cs_id,
        )
        .await?;

        Ok(rows
            .into_iter()
            .map(|x| MegarepoMappingEntry {
                source_name: SourceName::new(x.0),
                source_cs_id: source_cs_id.clone(),
                sync_config_version: x.2,
                target: target.clone(),
                target_cs_id: x.1,
            })
            .collect())
    }

    /// Add a mapping from a source commit to a target commit
    #[allow(clippy::ptr_arg)]
    pub async fn insert_source_target_cs_mapping(
        &self,
        ctx: &CoreContext,
        source_name: &SourceName,
        target: &Target,
        source_cs_id: ChangesetId,
        target_cs_id: ChangesetId,
        version: &SyncConfigVersion,
    ) -> Result<(), Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        let res = InsertMapping::maybe_traced_query(
            &self.connections.write_connection,
            ctx.client_request_info(),
            &[(
                source_name.as_str(),
                &target.repo_id,
                &target.bookmark,
                &source_cs_id,
                &target_cs_id,
                version,
            )],
        )
        .await?;
        if res.affected_rows() == 0 {
            // Because we insert to mapping before moving bookmark (which is fallible)
            // the mapping might be already inserted at that point. If it's the same
            // as what we wanted to insert we can ignore the failure to insert.
            if let Ok(Some(entry)) = self.get_mapping_entry(ctx, target, target_cs_id).await {
                if &entry.source_name != source_name
                    || entry.source_cs_id != source_cs_id
                    || &entry.sync_config_version != version
                {
                    return Err(anyhow!(
                        "trying to insert mapping while one already exists and it's different!"
                    ));
                }
            } else {
                return Err(anyhow!(
                    "unknown error while inserting mapping (affected_rows=0)"
                ));
            }
        }

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
    use fbinit::FacebookInit;
    use maplit::btreemap;
    use mononoke_macros::mononoke;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::THREES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;

    use super::*;

    #[mononoke::fbinit_test]
    async fn test_simple_mapping(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mapping = MegarepoMapping::with_sqlite_in_memory()?;

        let target = Target {
            repo_id: 0,
            bookmark: "book".to_string(),
            ..Default::default()
        };

        let source_csid = ONES_CSID;
        let target_csid = TWOS_CSID;
        let version = "version".to_string();

        mapping
            .insert_source_target_cs_mapping(
                &ctx,
                &SourceName::new("source_name"),
                &target,
                source_csid,
                target_csid,
                &version,
            )
            .await?;

        // Test to check if insertion is resilient against
        // the mapping being already there.
        mapping
            .insert_source_target_cs_mapping(
                &ctx,
                &SourceName::new("source_name"),
                &target,
                source_csid,
                target_csid,
                &version,
            )
            .await?;

        let res = mapping
            .get_mapping_entry(&ctx, &target, target_csid)
            .await?;

        assert_eq!(res.unwrap().sync_config_version, version);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_reverse_mapping(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mapping = MegarepoMapping::with_sqlite_in_memory()?;

        let target = Target {
            repo_id: 0,
            bookmark: "book".to_string(),
            ..Default::default()
        };

        let source_csid = ONES_CSID;
        let target_csid = TWOS_CSID;
        let version = "version".to_string();

        mapping
            .insert_source_target_cs_mapping(
                &ctx,
                &SourceName::new("source_name"),
                &target,
                source_csid,
                target_csid,
                &version,
            )
            .await?;

        let mut res = mapping
            .get_reverse_mapping_entry(&ctx, &target, source_csid)
            .await?;

        assert_eq!(res.len(), 1);

        assert_eq!(res.pop().unwrap().target_cs_id, target_csid);

        // query non-existent source_cs
        let res = mapping
            .get_reverse_mapping_entry(&ctx, &target, THREES_CSID)
            .await?;

        assert_eq!(res.len(), 0);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_serialize(_fb: FacebookInit) -> Result<(), Error> {
        let state = CommitRemappingState::new(
            btreemap! {
                SourceName::new("source_1") => ONES_CSID,
                SourceName::new("source_2") => TWOS_CSID,
            },
            "version_1".to_string(),
            None,
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

        let state: CommitRemappingState = serde_json::from_str(&res)?;
        assert_eq!(state.bookmark, None);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_deserialization_ignores_extra_fields(_fb: FacebookInit) -> Result<(), Error> {
        let s = r#"{
  "latest_synced_changesets": {
    "source_1": "1111111111111111111111111111111111111111111111111111111111111111",
    "source_2": "2222222222222222222222222222222222222222222222222222222222222222"
  },
  "sync_config_version": "version_1",
  "foo": 423423423
}"#;

        let state: CommitRemappingState = serde_json::from_str(s)?;
        assert_eq!(state.sync_config_version, "version_1");

        Ok(())
    }
}
