/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Result};
use futures::compat::Future01CompatExt;
use sql::queries;
use sql_ext::SqlConnections;

use stats::prelude::*;

use context::{CoreContext, PerfCounterType};
use mononoke_types::RepositoryId;

use crate::types::{DagBundle, IdDagVersion, IdMapVersion};

define_stats! {
    prefix = "mononoke.segmented_changelog.bundle";
    set: timeseries(Sum),
    get: timeseries(Sum),
}

/// Specifies the versions for the latest Dag bundle. The bundle contains IdDag and IdMap versions.
/// The IdDag version can be loaded directly from the blobstore and the IdMap version ties the
/// IdDag back to the bonsai changesets.
pub struct SqlBundleStore {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

impl SqlBundleStore {
    pub fn new(connections: SqlConnections, repo_id: RepositoryId) -> Self {
        Self {
            connections,
            repo_id,
        }
    }

    pub async fn set(&self, ctx: &CoreContext, bundle: DagBundle) -> Result<()> {
        STATS::set.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        InsertBundle::query(
            &self.connections.write_connection,
            &[(&self.repo_id, &bundle.iddag_version, &bundle.idmap_version)],
        )
        .compat()
        .await
        .context("inserting segmented changelog bundle")?;
        Ok(())
    }

    pub async fn get(&self, ctx: &CoreContext) -> Result<Option<DagBundle>> {
        STATS::get.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let rows = SelectBundle::query(&self.connections.read_connection, &self.repo_id)
            .compat()
            .await?;
        Ok(rows.into_iter().next().map(|r| r.into()))
    }
}

queries! {
    write InsertBundle(
        values: (repo_id: RepositoryId, iddag_version: IdDagVersion, idmap_version: IdMapVersion)
    ) {
        none,
        "
        REPLACE INTO segmented_changelog_bundle (repo_id, iddag_version, idmap_version)
        VALUES {values}
        "
    }

    read SelectBundle(repo_id: RepositoryId) -> (IdDagVersion, IdMapVersion) {
        "
        SELECT iddag_version, idmap_version
        FROM segmented_changelog_bundle
        WHERE repo_id = {repo_id}
        "
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use fbinit::FacebookInit;

    use sql_construct::SqlConstruct;

    use crate::builder::SegmentedChangelogBuilder;

    #[fbinit::compat_test]
    async fn test_more_than_one_repo(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let builder = SegmentedChangelogBuilder::with_sqlite_in_memory()?;
        let build_version = |id| {
            builder
                .clone()
                .with_repo_id(RepositoryId::new(id))
                .build_sql_bundle_store()
        };
        let version_repo1 = build_version(1)?;
        let version_repo2 = build_version(2)?;

        assert_eq!(version_repo1.get(&ctx).await?, None);
        assert_eq!(version_repo2.get(&ctx).await?, None);
        let bundle11 = DagBundle::new(IdDagVersion::from_serialized_bytes(b"1"), IdMapVersion(1));
        let bundle23 = DagBundle::new(IdDagVersion::from_serialized_bytes(b"2"), IdMapVersion(3));
        version_repo1.set(&ctx, bundle11).await?;
        assert_eq!(version_repo1.get(&ctx).await?, Some(bundle11));
        assert_eq!(version_repo2.get(&ctx).await?, None);
        version_repo2.set(&ctx, bundle23).await?;
        assert_eq!(version_repo1.get(&ctx).await?, Some(bundle11));
        assert_eq!(version_repo2.get(&ctx).await?, Some(bundle23));

        Ok(())
    }
}
