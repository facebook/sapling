/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use sql::queries;
use sql_ext::SqlConnections;

use stats::prelude::*;

use context::CoreContext;
use context::PerfCounterType;
use mononoke_types::RepositoryId;

use crate::logging::log_new_segmented_changelog_version;
use crate::types::IdDagVersion;
use crate::types::IdMapVersion;
use crate::types::SegmentedChangelogVersion;

define_stats! {
    prefix = "mononoke.segmented_changelog.sql_version_store";
    set: timeseries(Sum),
    update: timeseries(Sum),
    get: timeseries(Sum),
}

/// Specifies the versions for the latest SegmentedChangelogVersion. The version contains IdDag and
/// IdMap versions.  The IdDag version can be loaded directly from the blobstore and the IdMap
/// version ties the IdDag back to the bonsai changesets.
pub struct SegmentedChangelogVersionStore {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

impl SegmentedChangelogVersionStore {
    pub fn new(connections: SqlConnections, repo_id: RepositoryId) -> Self {
        Self {
            connections,
            repo_id,
        }
    }

    pub async fn set(&self, ctx: &CoreContext, version: SegmentedChangelogVersion) -> Result<()> {
        STATS::set.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        SetVersion::query(
            &self.connections.write_connection,
            &self.repo_id,
            &version.iddag_version,
            &version.idmap_version,
        )
        .await
        .with_context(|| format!("failed to set segmented changelog version {:?}", version))?;
        log_new_segmented_changelog_version(ctx, self.repo_id, version);
        Ok(())
    }

    pub async fn update(
        &self,
        ctx: &CoreContext,
        version: SegmentedChangelogVersion,
    ) -> Result<()> {
        STATS::update.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        let result = UpdateVersion::query(
            &self.connections.write_connection,
            &self.repo_id,
            &version.iddag_version,
            &version.idmap_version,
        )
        .await
        .with_context(|| {
            format!(
                "failed to update segmented changelog version ({:?})",
                version
            )
        })?;
        if result.affected_rows() == 0 {
            bail!(
                "no valid rows to update for segmented changelog version {:?}",
                version
            );
        }
        log_new_segmented_changelog_version(ctx, self.repo_id, version);
        Ok(())
    }

    pub async fn get(&self, ctx: &CoreContext) -> Result<Option<SegmentedChangelogVersion>> {
        STATS::get.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let rows = SelectVersion::query(&self.connections.read_connection, &self.repo_id).await?;
        Ok(rows.into_iter().next().map(|r| r.into()))
    }
}

queries! {
    write SetVersion(
        repo_id: RepositoryId,
        iddag_version: IdDagVersion,
        idmap_version: IdMapVersion
    ) {
        none,
        "
        REPLACE INTO segmented_changelog_version (repo_id, iddag_version, idmap_version)
        VALUES ({repo_id}, {iddag_version}, {idmap_version})
        "
    }

    write UpdateVersion(
        repo_id: RepositoryId,
        iddag_version: IdDagVersion,
        idmap_version: IdMapVersion,
    ) {
        none,
        "
        UPDATE segmented_changelog_version
        SET iddag_version = {iddag_version}
        WHERE repo_id = {repo_id} AND idmap_version = {idmap_version}
        "
    }

    read SelectVersion(repo_id: RepositoryId) -> (IdDagVersion, IdMapVersion) {
        "
        SELECT iddag_version, idmap_version
        FROM segmented_changelog_version
        WHERE repo_id = {repo_id}
        "
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use fbinit::FacebookInit;

    use sql_construct::SqlConstruct;

    use crate::builder::SegmentedChangelogSqlConnections;

    #[fbinit::test]
    async fn test_set_more_than_one_repo(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;
        let version_repo1 =
            SegmentedChangelogVersionStore::new(conns.0.clone(), RepositoryId::new(1));
        let version_repo2 =
            SegmentedChangelogVersionStore::new(conns.0.clone(), RepositoryId::new(2));

        assert_eq!(version_repo1.get(&ctx).await?, None);
        assert_eq!(version_repo2.get(&ctx).await?, None);
        let version11 = SegmentedChangelogVersion::new(
            IdDagVersion::from_serialized_bytes(b"1"),
            IdMapVersion(1),
        );
        let version23 = SegmentedChangelogVersion::new(
            IdDagVersion::from_serialized_bytes(b"2"),
            IdMapVersion(3),
        );
        version_repo1.set(&ctx, version11).await?;
        assert_eq!(version_repo1.get(&ctx).await?, Some(version11));
        assert_eq!(version_repo2.get(&ctx).await?, None);
        version_repo2.set(&ctx, version23).await?;
        assert_eq!(version_repo1.get(&ctx).await?, Some(version11));
        assert_eq!(version_repo2.get(&ctx).await?, Some(version23));

        Ok(())
    }

    #[fbinit::test]
    async fn test_update(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;
        let version_store = SegmentedChangelogVersionStore::new(conns.0, RepositoryId::new(0));

        let vm1 = SegmentedChangelogVersion::new(
            IdDagVersion::from_serialized_bytes(b"a"),
            IdMapVersion(1),
        );
        let vm1x = SegmentedChangelogVersion::new(
            IdDagVersion::from_serialized_bytes(b"x"),
            IdMapVersion(1),
        );
        let vm2 = SegmentedChangelogVersion::new(
            IdDagVersion::from_serialized_bytes(b"b"),
            IdMapVersion(2),
        );
        let vm2y = SegmentedChangelogVersion::new(
            IdDagVersion::from_serialized_bytes(b"y"),
            IdMapVersion(2),
        );

        version_store.set(&ctx, vm1).await?;
        assert_eq!(version_store.get(&ctx).await?, Some(vm1));
        assert!(version_store.update(&ctx, vm2y).await.is_err());
        version_store.update(&ctx, vm1x).await?;
        assert_eq!(version_store.get(&ctx).await?, Some(vm1x));
        version_store.set(&ctx, vm2).await?;
        assert!(version_store.update(&ctx, vm1x).await.is_err());
        assert_eq!(version_store.get(&ctx).await?, Some(vm2));
        version_store.update(&ctx, vm2y).await?;
        assert_eq!(version_store.get(&ctx).await?, Some(vm2y));

        Ok(())
    }
}
