/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkName;
use context::CoreContext;
use megarepo_configs::Source;
use megarepo_configs::SyncConfigVersion;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;
use sql_ext::mononoke_queries;
use tracing::warn;

use crate::db::MegarepoSyncConfig;
use crate::db::types::MegarepoSyncConfigEntry;
use crate::db::types::RowId;

/// When true, re-inserting different content under an existing
/// `(repo_id, bookmark, version)` key is an error. When false it succeeds
/// but keeps the pre-existing row.
const REJECT_DIVERGENT_REINSERT_JK: &str = "scm/mononoke:megarepo_reject_divergent_config_reinsert";

mononoke_queries! {
    // Insert for one (repo_id, bookmark, version) key. Once the UNIQUE index
    // on (repo_id, bookmark, version) lands (T236275292), a repeated insert
    // becomes a no-op: MySQL via ON DUPLICATE KEY UPDATE, SQLite via
    // INSERT OR IGNORE. Until then this is a plain insert and `add_repo_config`
    // dedups in application code before inserting (see below).
    // Callers must still read the row back (see `add_repo_config`) to learn
    // its id and to detect divergent content.
    write AddRepoConfig(repo_id: RepositoryId, bookmark: BookmarkName, version: SyncConfigVersion, serialized_config: &str) {
        none,
        mysql("INSERT INTO megarepo_sync_config
        (`repo_id`, `bookmark`, `version`, `serialized_config`)
         VALUES ({repo_id}, {bookmark}, {version}, {serialized_config})
         ON DUPLICATE KEY UPDATE `repo_id` = `repo_id`
        ")
        sqlite("INSERT OR IGNORE INTO megarepo_sync_config
        (`repo_id`, `bookmark`, `version`, `serialized_config`)
         VALUES ({repo_id}, {bookmark}, {version}, {serialized_config})
        ")
    }

    read TestGetRepoConfigById(id: RowId)  -> (
        RowId,
        RepositoryId,
        BookmarkName,
        SyncConfigVersion,
        String,
    ) {
        "SELECT id, repo_id, bookmark, version, serialized_config
        FROM megarepo_sync_config
        WHERE id = {id}
        "
    }

    read GetRepoConfigByVersion(repo_id: RepositoryId, bookmark: BookmarkName, version: SyncConfigVersion) -> (
        RowId,
        RepositoryId,
        BookmarkName,
        SyncConfigVersion,
        String,
    ) {
        "SELECT id, repo_id, bookmark, version, serialized_config
        FROM megarepo_sync_config
        WHERE repo_id = {repo_id} AND bookmark = {bookmark} AND version = {version}
        ORDER BY id
        LIMIT 1
        "
    }

    // All rows for one key, oldest first. `add_repo_config` reads these back
    // to find the row holding its content.
    read GetRepoConfigsByKey(repo_id: RepositoryId, bookmark: BookmarkName, version: SyncConfigVersion) -> (RowId, String) {
        "SELECT id, serialized_config
        FROM megarepo_sync_config
        WHERE repo_id = {repo_id} AND bookmark = {bookmark} AND version = {version}
        ORDER BY id
        "
    }
}

fn row_to_entry(
    row: (RowId, RepositoryId, BookmarkName, SyncConfigVersion, String),
) -> Result<MegarepoSyncConfigEntry> {
    let (id, repo_id, bookmark, version, contents) = row;
    let sources: Vec<Source> = fbthrift::simplejson_protocol::deserialize(contents)
        .context("failed to deserialize existing config")?;
    Ok(MegarepoSyncConfigEntry {
        id,
        repo_id,
        bookmark,
        version,
        sources,
    })
}

#[derive(Clone)]
pub struct SqlMegarepoSyncConfig {
    pub(crate) connections: SqlConnections,
}

#[async_trait]
impl MegarepoSyncConfig for SqlMegarepoSyncConfig {
    async fn add_repo_config(
        &self,
        ctx: &CoreContext,
        repo_id: &RepositoryId,
        bookmark: &BookmarkKey,
        version: &SyncConfigVersion,
        sources: Vec<Source>,
    ) -> Result<RowId> {
        let contents =
            String::from_utf8(fbthrift::simplejson_protocol::serialize(&sources).to_vec())
                .context("failed to serialize SyncTargetConfig")?;
        // TEMPORARY until the UNIQUE constraint lands (T236275292): prod has
        // no UNIQUE index on (repo_id, bookmark, version) yet, so the insert
        // below is a plain insert there. Dedup in application code instead:
        // read existing rows on the write connection (no replica lag) and
        // only insert when the key has no row at all. Identical content
        // always returns the existing row; divergent content is rejected
        // only when the JustKnob is enabled.
        let reject_divergent = justknobs::eval(REJECT_DIVERGENT_REINSERT_JK, None, None);
        let rows = GetRepoConfigsByKey::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            repo_id,
            bookmark.name(),
            version,
        )
        .await?;
        if let Some((id, _)) = rows.iter().find(|(_, existing)| *existing == contents) {
            return Ok(*id);
        }
        if let Some((id, _)) = rows.into_iter().next() {
            if reject_divergent {
                bail!(
                    "refusing to overwrite megarepo_sync_config for {repo_id} {bookmark} {version}: a row with different content already exists"
                );
            }
            warn!(
                "megarepo_sync_config for {repo_id} {bookmark} {version} already exists with different content; keeping pre-existing row {id}"
            );
            return Ok(id);
        }
        AddRepoConfig::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            repo_id,
            bookmark.name(),
            version,
            &contents.as_str(),
        )
        .await?;

        // Read back on the write connection so the row is visible without
        // waiting for replica lag. A repeated insert is a no-op once the
        // UNIQUE index lands (see `AddRepoConfig`); until then a concurrent
        // writer may have inserted first, so find the row holding our
        // content.
        let rows = GetRepoConfigsByKey::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            repo_id,
            bookmark.name(),
            version,
        )
        .await?;
        if let Some((id, _)) = rows.iter().find(|(_, existing)| *existing == contents) {
            return Ok(*id);
        }
        // No row holds our content but another row exists for the key: a
        // concurrent writer won the race.
        match rows.into_iter().next() {
            Some((id, _)) => {
                if reject_divergent {
                    bail!(
                        "refusing to overwrite megarepo_sync_config for {repo_id} {bookmark} {version}: a row with different content already exists"
                    );
                }
                warn!(
                    "megarepo_sync_config for {repo_id} {bookmark} {version} already exists with different content; keeping pre-existing row {id}"
                );
                Ok(id)
            }
            None => bail!("Failed to insert a repo config for {repo_id} {bookmark} {version}"),
        }
    }

    #[cfg(test)]
    async fn test_get_repo_config_by_id(
        &self,
        ctx: &CoreContext,
        id: &RowId,
    ) -> Result<Option<MegarepoSyncConfigEntry>> {
        let rows = TestGetRepoConfigById::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            id,
        )
        .await?;
        match rows.into_iter().next() {
            None => Ok(None),
            Some(row) => Ok(Some(row_to_entry(row)?)),
        }
    }

    async fn get_repo_config_by_version(
        &self,
        ctx: &CoreContext,
        repo_id: &RepositoryId,
        bookmark: &BookmarkKey,
        version: &SyncConfigVersion,
    ) -> Result<Option<MegarepoSyncConfigEntry>> {
        let rows = GetRepoConfigByVersion::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            repo_id,
            bookmark.name(),
            version,
        )
        .await?;
        match rows.into_iter().next() {
            None => Ok(None),
            Some(row) => Ok(Some(row_to_entry(row)?)),
        }
    }
}

impl SqlConstruct for SqlMegarepoSyncConfig {
    const LABEL: &'static str = "megarepo_sync_config";

    const CREATION_QUERY: &'static str =
        include_str!("../../schemas/sqlite-megarepo_sync_config.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlMegarepoSyncConfig {
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

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use fbinit::FacebookInit;
    use justknobs::test_helpers::JustKnobsInMemory;
    use justknobs::test_helpers::KnobVal;
    use justknobs::test_helpers::with_just_knobs_async;
    use megarepo_configs::SourceMappingRules;
    use megarepo_configs::SourceRevision;
    use mononoke_macros::mononoke;

    use super::*;

    fn test_source(name: &str) -> Source {
        Source {
            source_name: name.to_string(),
            repo_id: 1,
            name: "gitrepo".to_string(),
            revision: SourceRevision::bookmark("main".to_string()),
            mapping: SourceMappingRules {
                default_prefix: name.to_string(),
                linkfiles: Default::default(),
                overrides: Default::default(),
            },
            merge_mode: None,
        }
    }

    fn reject_divergent_jk(enabled: bool) -> JustKnobsInMemory {
        JustKnobsInMemory::new(HashMap::from([(
            REJECT_DIVERGENT_REINSERT_JK.to_string(),
            KnobVal::Bool(enabled),
        )]))
    }

    #[mononoke::fbinit_test]
    async fn test_add_repo_config(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlMegarepoSyncConfig::with_sqlite_in_memory()?;
        let id = config
            .add_repo_config(
                &ctx,
                &RepositoryId::new(0),
                &BookmarkKey::new("book")?,
                &"12345678".to_string(),
                vec![],
            )
            .await?;

        let entry = config.test_get_repo_config_by_id(&ctx, &id).await?;
        assert!(entry.is_some());

        let entry = entry.unwrap();
        assert_eq!(entry.repo_id, RepositoryId::new(0));
        assert_eq!(entry.bookmark, *BookmarkKey::new("book")?.name());
        assert_eq!(entry.version, "12345678");
        assert_eq!(entry.sources, vec![]);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_get_config_by_version(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlMegarepoSyncConfig::with_sqlite_in_memory()?;
        config
            .add_repo_config(
                &ctx,
                &RepositoryId::new(0),
                &BookmarkKey::new("book")?,
                &"12345678".to_string(),
                vec![],
            )
            .await?;

        let entry = config
            .get_repo_config_by_version(
                &ctx,
                &RepositoryId::new(404),
                &BookmarkKey::new("book")?,
                &"12345678".to_string(),
            )
            .await?;
        assert!(entry.is_none());

        let entry = config
            .get_repo_config_by_version(
                &ctx,
                &RepositoryId::new(0),
                &BookmarkKey::new("book")?,
                &"12345678".to_string(),
            )
            .await?;
        assert!(entry.is_some());

        let entry = entry.unwrap();
        assert_eq!(entry.repo_id, RepositoryId::new(0));
        assert_eq!(entry.bookmark, *BookmarkKey::new("book")?.name());
        assert_eq!(entry.version, "12345678");
        assert_eq!(entry.sources, vec![]);

        Ok(())
    }

    /// Retrying an insert with identical content must not create a duplicate
    /// row: both calls succeed with the same row id.
    #[mononoke::fbinit_test]
    async fn test_add_repo_config_duplicate_identical(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlMegarepoSyncConfig::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(0);
        let bookmark = BookmarkKey::new("book")?;
        let version = "12345678".to_string();
        let sources = vec![test_source("a")];

        let id1 = config
            .add_repo_config(&ctx, &repo_id, &bookmark, &version, sources.clone())
            .await?;
        let id2 = config
            .add_repo_config(&ctx, &repo_id, &bookmark, &version, sources.clone())
            .await?;
        assert_eq!(id1, id2);

        let entry = config
            .get_repo_config_by_version(&ctx, &repo_id, &bookmark, &version)
            .await?
            .expect("config must exist");
        assert_eq!(entry.id, id1);
        assert_eq!(entry.sources, sources);

        // The retry must not leave a duplicate row behind. The test sqlite
        // DB has no UNIQUE index (like prod), so this proves
        // application-level dedup.
        let rows = GetRepoConfigsByKey::query(
            &config.connections.write_connection,
            ctx.sql_query_telemetry(),
            &repo_id,
            bookmark.name(),
            &version,
        )
        .await?;
        assert_eq!(rows.len(), 1);

        Ok(())
    }

    /// With the knob off (shadow mode), re-inserting different content
    /// succeeds but keeps the pre-existing row without inserting a duplicate.
    #[mononoke::fbinit_test]
    async fn test_add_repo_config_divergent_shadow_mode(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlMegarepoSyncConfig::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(0);
        let bookmark = BookmarkKey::new("book")?;
        let version = "12345678".to_string();

        let id1 = config
            .add_repo_config(&ctx, &repo_id, &bookmark, &version, vec![])
            .await?;

        with_just_knobs_async(
            reject_divergent_jk(false),
            Box::pin(async {
                let id2 = config
                    .add_repo_config(&ctx, &repo_id, &bookmark, &version, vec![test_source("a")])
                    .await
                    .expect("divergent re-insert must succeed in shadow mode");
                assert_eq!(id1, id2);

                let entry = config
                    .get_repo_config_by_version(&ctx, &repo_id, &bookmark, &version)
                    .await
                    .expect("read must succeed")
                    .expect("config must exist");
                assert_eq!(entry.sources, vec![]);
            }),
        )
        .await;

        // Shadow mode must not insert a duplicate row either: it keeps the
        // pre-existing row and returns its id.
        let rows = GetRepoConfigsByKey::query(
            &config.connections.write_connection,
            ctx.sql_query_telemetry(),
            &repo_id,
            bookmark.name(),
            &version,
        )
        .await?;
        assert_eq!(rows.len(), 1);

        Ok(())
    }

    /// With the knob on, re-inserting different content is rejected and the
    /// pre-existing row is left untouched.
    #[mononoke::fbinit_test]
    async fn test_add_repo_config_divergent_rejected(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlMegarepoSyncConfig::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(0);
        let bookmark = BookmarkKey::new("book")?;
        let version = "12345678".to_string();

        let id1 = config
            .add_repo_config(&ctx, &repo_id, &bookmark, &version, vec![])
            .await?;

        with_just_knobs_async(
            reject_divergent_jk(true),
            Box::pin(async {
                let err = config
                    .add_repo_config(&ctx, &repo_id, &bookmark, &version, vec![test_source("a")])
                    .await
                    .expect_err("divergent re-insert must be rejected with the knob on");
                assert!(
                    err.to_string().contains("refusing to overwrite"),
                    "unexpected error: {err:?}"
                );

                let entry = config
                    .get_repo_config_by_version(&ctx, &repo_id, &bookmark, &version)
                    .await
                    .expect("read must succeed")
                    .expect("config must exist");
                assert_eq!(entry.id, id1);
                assert_eq!(entry.sources, vec![]);
            }),
        )
        .await;

        Ok(())
    }

    /// With the knob on, a retry finds its row in the pre-insert check and
    /// returns it without inserting.
    #[mononoke::fbinit_test]
    async fn test_add_repo_config_retry_noop_when_knob_on(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let config = SqlMegarepoSyncConfig::with_sqlite_in_memory()?;
        let repo_id = RepositoryId::new(0);
        let bookmark = BookmarkKey::new("book")?;
        let version = "12345678".to_string();
        let sources = vec![test_source("a")];

        with_just_knobs_async(
            reject_divergent_jk(true),
            Box::pin(async {
                let id1 = config
                    .add_repo_config(&ctx, &repo_id, &bookmark, &version, sources.clone())
                    .await
                    .expect("first insert must succeed");
                let id2 = config
                    .add_repo_config(&ctx, &repo_id, &bookmark, &version, sources.clone())
                    .await
                    .expect("retry must succeed without inserting");
                assert_eq!(id1, id2);

                let entry = config
                    .get_repo_config_by_version(&ctx, &repo_id, &bookmark, &version)
                    .await
                    .expect("read must succeed")
                    .expect("config must exist");
                assert_eq!(entry.sources, sources);
            }),
        )
        .await;

        Ok(())
    }
}
