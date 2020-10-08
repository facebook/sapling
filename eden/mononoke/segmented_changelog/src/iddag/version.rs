/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Context, Result};
use futures::compat::Future01CompatExt;
use sql::queries;
use sql_ext::SqlConnections;

use stats::prelude::*;

use context::{CoreContext, PerfCounterType};
use mononoke_types::RepositoryId;

use crate::types::{IdDagVersion, IdMapVersion};

define_stats! {
    prefix = "mononoke.segmented_changelog.iddag.version";
    new_version: timeseries(Sum),
}

/// This structure just generates new versions to the IdDag to be saved under. It also serves as a
/// log for when IdDags completed their computations. There are many other options for generating
/// the version of an IdDag, hashing the content for example, this one is just what seemed most
/// convenient.
pub struct SqlIdDagVersionStore {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

impl SqlIdDagVersionStore {
    pub fn new(connections: SqlConnections, repo_id: RepositoryId) -> Self {
        Self {
            connections,
            repo_id,
        }
    }

    pub async fn new_version(
        &self,
        ctx: &CoreContext,
        idmap_version: IdMapVersion,
    ) -> Result<IdDagVersion> {
        STATS::new_version.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        let result = InsertVersionLog::query(
            &self.connections.write_connection,
            &[(&self.repo_id, &idmap_version)],
        )
        .compat()
        .await
        .context("inserting segmented changelog iddag version log entry")?;

        let last_id = result.last_insert_id().ok_or_else(|| {
            format_err!("no rows inserted in segmented changelog iddag version log")
        })?;
        Ok(IdDagVersion(last_id))
    }
}

queries! {
    write InsertVersionLog(values: (repo_id: RepositoryId, idmap_version: IdMapVersion)) {
        none,
        "
        INSERT INTO segmented_changelog_iddag_version_log (repo_id, idmap_version)
        VALUES {values}
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
    async fn test_new_version(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let builder = SegmentedChangelogBuilder::with_sqlite_in_memory()?;
        let build_version = |id| {
            builder
                .clone()
                .with_repo_id(RepositoryId::new(id))
                .build_sql_iddag_version_store()
        };
        let version_repo1 = build_version(1)?;
        let version_repo2 = build_version(2)?;
        assert_eq!(
            version_repo1.new_version(&ctx, IdMapVersion(2)).await?,
            IdDagVersion(1)
        );
        assert_eq!(
            version_repo1.new_version(&ctx, IdMapVersion(2)).await?,
            IdDagVersion(2)
        );
        assert_eq!(
            version_repo2.new_version(&ctx, IdMapVersion(1)).await?,
            IdDagVersion(3)
        );
        assert_eq!(
            version_repo2.new_version(&ctx, IdMapVersion(2)).await?,
            IdDagVersion(4)
        );
        Ok(())
    }
}
