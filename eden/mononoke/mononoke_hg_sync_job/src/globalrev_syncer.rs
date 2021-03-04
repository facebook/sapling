/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use context::CoreContext;
use fbinit::FacebookInit;
use metaconfig_types::HgsqlGlobalrevsName;
use mononoke_types::ChangesetId;
use sql::{queries, Connection};
use sql_construct::{facebook::FbSqlConstruct, SqlConstruct};
use sql_ext::{facebook::MysqlOptions, SqlConnections};
use std::path::Path;
use std::sync::Arc;

#[derive(Clone)]
pub enum GlobalrevSyncer {
    Noop,
    Sql(Arc<SqlGlobalrevSyncer>),
}

pub struct SqlGlobalrevSyncer {
    hgsql_name: HgsqlGlobalrevsName,
    repo: BlobRepo,
    hgsql: HgsqlConnection,
    globalrevs_publishing_bookmark: BookmarkName,
}

#[derive(Clone)]
struct HgsqlConnection {
    connection: Connection,
}

impl SqlConstruct for HgsqlConnection {
    const LABEL: &'static str = "globalrev-syncer";

    const CREATION_QUERY: &'static str = include_str!("../schemas/hgsql.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            connection: connections.write_connection,
        }
    }
}

impl GlobalrevSyncer {
    pub async fn new(
        fb: FacebookInit,
        repo: BlobRepo,
        use_sqlite: bool,
        params: Option<(&str, BookmarkName)>,
        mysql_options: &MysqlOptions,
        readonly: bool,
        hgsql_name: HgsqlGlobalrevsName,
    ) -> Result<Self, Error> {
        let (hgsql_db_addr, globalrevs_publishing_bookmark) = match params {
            Some((hgsql_db_addr, globalrevs_publishing_bookmark)) => {
                (hgsql_db_addr, globalrevs_publishing_bookmark)
            }
            None => return Ok(GlobalrevSyncer::Noop),
        };

        let hgsql = if use_sqlite {
            HgsqlConnection::with_sqlite_path(Path::new(hgsql_db_addr), readonly)?
        } else {
            HgsqlConnection::with_xdb(fb, hgsql_db_addr.to_string(), &mysql_options, readonly)
                .await?
        };

        let syncer = SqlGlobalrevSyncer {
            hgsql_name,
            repo,
            hgsql,
            globalrevs_publishing_bookmark,
        };

        Ok(GlobalrevSyncer::Sql(Arc::new(syncer)))
    }

    pub async fn sync(
        &self,
        ctx: &CoreContext,
        bookmark: &BookmarkName,
        bcs_id: ChangesetId,
    ) -> Result<(), Error> {
        match self {
            Self::Noop => Ok(()),
            Self::Sql(syncer) => syncer.sync(ctx, bookmark, bcs_id).await,
        }
    }
}

impl SqlGlobalrevSyncer {
    pub async fn sync(
        &self,
        ctx: &CoreContext,
        bookmark: &BookmarkName,
        bcs_id: ChangesetId,
    ) -> Result<(), Error> {
        if *bookmark != self.globalrevs_publishing_bookmark {
            return Ok(());
        }

        let rev = self
            .repo
            .get_globalrev_from_bonsai(ctx, bcs_id)
            .await?
            .ok_or_else(|| format_err!("Globalrev is missing for bcs_id = {}", bcs_id))?
            .id()
            + 1;

        let rows =
            IncreaseGlobalrevCounter::query(&self.hgsql.connection, self.hgsql_name.as_ref(), &rev)
                .await?
                .affected_rows();

        if rows > 0 {
            return Ok(());
        }

        // If the counter is already where we want it do be, then we won't actually modify the row,
        // and affected_rows will return 0. The right way to fix this would be to set
        // CLIENT_FOUND_ROWS when connecting to MySQL and use value <= rev so that affected_rows
        // tells us about rows it found as opposed to rows actually modified (which is how SQLite
        // would behave locally). However, for now let's do the more expedient thing and just have
        // both MySQL and SQLite behave the same by avoiding no-op updates. This makes this logic
        // easier to unit test.

        let db_rev = GetGlobalrevCounter::query(&self.hgsql.connection, self.hgsql_name.as_ref())
            .await?
            .into_iter()
            .next()
            .map(|r| r.0);

        if let Some(db_rev) = db_rev {
            if db_rev == rev {
                return Ok(());
            }
        }

        Err(format_err!(
            "Attempted to move Globalrev for repository {:?} backwards to {} (from {:?})",
            self.hgsql_name,
            rev,
            db_rev,
        ))
    }
}

queries! {
    write IncreaseGlobalrevCounter(repo: String, rev: u64) {
        none,
        "
        UPDATE revision_references
        SET value = {rev}
        WHERE repo = {repo}
          AND namespace = 'counter'
          AND name = 'commit'
          AND value < {rev}
        "
    }

    read GetGlobalrevCounter(repo: String) -> (u64) {
        "
        SELECT value FROM revision_references
        WHERE repo = {repo}
          AND namespace = 'counter'
          AND name = 'commit'
        "
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use bonsai_globalrev_mapping::BonsaiGlobalrevMappingEntry;
    use mercurial_types_mocks::globalrev::{GLOBALREV_ONE, GLOBALREV_THREE, GLOBALREV_TWO};
    use mononoke_types_mocks::changesetid::{ONES_CSID, TWOS_CSID};
    use mononoke_types_mocks::repo::REPO_ZERO;
    use sql::rusqlite::Connection as SqliteConnection;

    queries! {
        write InitGlobalrevCounter(repo: String, rev: u64) {
            none,
            "
            INSERT INTO revision_references(repo, namespace, name, value)
            VALUES ({repo}, 'counter', 'commit', {rev})
            "
        }
    }

    #[fbinit::test]
    fn test_sync(fb: FacebookInit) -> Result<(), Error> {
        async_unit::tokio_unit_test(async move {
            let ctx = CoreContext::test_mock(fb);

            let master = BookmarkName::new("master")?;
            let stable = BookmarkName::new("stable")?;

            let sqlite = SqliteConnection::open_in_memory()?;
            sqlite.execute_batch(HgsqlConnection::CREATION_QUERY)?;
            let connection = Connection::with_sqlite(sqlite);

            let repo = blobrepo_factory::new_memblob_empty(None)?;
            let hgsql_name = HgsqlGlobalrevsName("foo".to_string());

            let e1 = BonsaiGlobalrevMappingEntry {
                repo_id: REPO_ZERO,
                bcs_id: ONES_CSID,
                globalrev: GLOBALREV_ONE,
            };

            let e2 = BonsaiGlobalrevMappingEntry {
                repo_id: REPO_ZERO,
                bcs_id: TWOS_CSID,
                globalrev: GLOBALREV_TWO,
            };

            repo.bonsai_globalrev_mapping()
                .bulk_import(&ctx, &[e1, e2])
                .await?;

            let syncer = SqlGlobalrevSyncer {
                hgsql_name: hgsql_name.clone(),
                repo,
                hgsql: HgsqlConnection {
                    connection: connection.clone(),
                },
                globalrevs_publishing_bookmark: master.clone(),
            };

            // First, check that setting a globalrev before the counter exists fails.
            assert!(syncer.sync(&ctx, &master, ONES_CSID).await.is_err());

            // Now, set the counter

            InitGlobalrevCounter::query(&connection, hgsql_name.as_ref(), &0).await?;

            // Now, try again to set the globalrev

            syncer.sync(&ctx, &master, TWOS_CSID).await?;

            assert_eq!(
                GetGlobalrevCounter::query(&connection, hgsql_name.as_ref())
                    .await?
                    .into_iter()
                    .next()
                    .ok_or_else(|| Error::msg("Globalrev missing"))?
                    .0,
                GLOBALREV_THREE.id()
            );

            // Check that we can sync the same value again successfully

            syncer.sync(&ctx, &master, TWOS_CSID).await?;

            // Check that we can't move it back

            assert!(syncer.sync(&ctx, &master, ONES_CSID).await.is_err());

            assert_eq!(
                GetGlobalrevCounter::query(&connection, hgsql_name.as_ref())
                    .await?
                    .into_iter()
                    .next()
                    .ok_or_else(|| Error::msg("Globalrev missing"))?
                    .0,
                GLOBALREV_THREE.id()
            );

            // Check that moving a non-publishing bookmark works, but doesn't touch the counter.

            syncer.sync(&ctx, &stable, ONES_CSID).await?;

            assert_eq!(
                GetGlobalrevCounter::query(&connection, hgsql_name.as_ref())
                    .await?
                    .into_iter()
                    .next()
                    .ok_or_else(|| Error::msg("Globalrev missing"))?
                    .0,
                GLOBALREV_THREE.id()
            );

            Ok(())
        })
    }
}
