/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Result};
use sql::queries;
use sql_ext::SqlConnections;

use stats::prelude::*;

use context::{CoreContext, PerfCounterType};
use mononoke_types::RepositoryId;

use crate::logging::log_new_idmap_version;
use crate::types::IdMapVersion;

define_stats! {
    prefix = "mononoke.segmented_changelog.idmap.version";
    set: timeseries(Sum),
    get: timeseries(Sum),
}

/// Describes the latest IdMap version for an given repository.
/// The seeder process has the job of constructing a new IdMap version. The seeder process will
/// insert entries for a new version and in the end it will set a new entry for a given repository.
/// Serving processes will use the SegmentedChangelogVersion under normal circumstances. Tailing
/// processes will read the latest version for a repository to incrementally build (tailers may or
/// may not use bundles).
pub struct SqlIdMapVersionStore {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

impl SqlIdMapVersionStore {
    pub fn new(connections: SqlConnections, repo_id: RepositoryId) -> Self {
        Self {
            connections,
            repo_id,
        }
    }

    pub async fn set(&self, ctx: &CoreContext, idmap_version: IdMapVersion) -> Result<()> {
        STATS::set.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        InsertVersion::query(
            &self.connections.write_connection,
            &[(&self.repo_id, &idmap_version)],
        )
        .await
        .context("inserting segmented changelog idmap version")?;

        log_new_idmap_version(ctx, self.repo_id, idmap_version);

        Ok(())
    }

    pub async fn get(&self, ctx: &CoreContext) -> Result<Option<IdMapVersion>> {
        STATS::get.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let rows = SelectVersion::query(&self.connections.read_connection, &self.repo_id).await?;
        Ok(rows.into_iter().next().map(|(v,)| v))
    }
}

queries! {
    write InsertVersion(values: (repo_id: RepositoryId, version: IdMapVersion)) {
        none,
        "
        REPLACE INTO segmented_changelog_idmap_version (repo_id, version)
        VALUES {values}
        "
    }

    read SelectVersion(repo_id: RepositoryId) -> (IdMapVersion) {
        "
        SELECT version
        FROM segmented_changelog_idmap_version
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
    async fn test_get_set_get(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo_id = RepositoryId::new(0);
        let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;
        let version = SqlIdMapVersionStore::new(conns.0, repo_id);

        assert_eq!(version.get(&ctx).await?, None);
        version.set(&ctx, IdMapVersion(1)).await?;
        assert_eq!(version.get(&ctx).await?, Some(IdMapVersion(1)));
        version.set(&ctx, IdMapVersion(3)).await?;
        assert_eq!(version.get(&ctx).await?, Some(IdMapVersion(3)));

        Ok(())
    }

    #[fbinit::test]
    async fn test_more_than_one_repo(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;

        let version_repo1 = SqlIdMapVersionStore::new(conns.0.clone(), RepositoryId::new(1));
        let version_repo2 = SqlIdMapVersionStore::new(conns.0.clone(), RepositoryId::new(2));

        assert_eq!(version_repo1.get(&ctx).await?, None);
        assert_eq!(version_repo2.get(&ctx).await?, None);
        version_repo1.set(&ctx, IdMapVersion(1)).await?;
        assert_eq!(version_repo1.get(&ctx).await?, Some(IdMapVersion(1)));
        assert_eq!(version_repo2.get(&ctx).await?, None);
        version_repo2.set(&ctx, IdMapVersion(1)).await?;
        assert_eq!(version_repo1.get(&ctx).await?, Some(IdMapVersion(1)));
        assert_eq!(version_repo2.get(&ctx).await?, Some(IdMapVersion(1)));
        version_repo2.set(&ctx, IdMapVersion(2)).await?;
        assert_eq!(version_repo1.get(&ctx).await?, Some(IdMapVersion(1)));
        assert_eq!(version_repo2.get(&ctx).await?, Some(IdMapVersion(2)));

        Ok(())
    }
}
