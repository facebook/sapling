/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use ascii::AsciiString;
use bookmarks::BookmarkName;
use chrono::naive::NaiveDateTime;
use futures::compat::Future01CompatExt;
use mercurial_types::HgChangesetId;
use sql::{queries, Connection};
use sql_construct::SqlConstruct;
use sql_ext::SqlConnections;
use std::collections::hash_map::HashMap;

pub type Entry = (i64, HgChangesetId, NaiveDateTime);
#[derive(Debug, PartialEq, Eq)]
pub struct BookmarkBatch {
    pub dt: NaiveDateTime,
    pub entries: Vec<Entry>,
}
pub type Batch = HashMap<BookmarkName, BookmarkBatch>;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct QueueLimit(pub usize);

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct Backfill(pub bool);

const DATE_TIME_FORMAT: &'static str = "%Y-%m-%d %H:%M:%S";

queries! {
    // NOTE: For now, we don't actually limit results here, since we expect fairly low volume on
    // this, and wouldn't want to have a bad batch of bookmarks at the bottom of the queue block
    // the rest. If that becomes a problem, we can a) shard on BookmarkName, and b) update this
    // code to have a limit, as long as we coalesce bookmarks we failed to sync in some way.
    read FetchQueue(repo_name: String, backfill: i64, limit: usize) -> (i64, BookmarkName, String, String) {
        "SELECT id, bookmark, node, created_at
         FROM replaybookmarksqueue
         WHERE reponame = {repo_name} AND synced = 0 AND backfill = {backfill}
         ORDER BY id ASC
         LIMIT {limit}"
    }

    write ReleaseEntries(>list ids: i64) {
        none,
        "UPDATE replaybookmarksqueue SET synced = 1 WHERE id IN {ids}"
    }
}

#[derive(Clone)]
pub struct SqlReplayBookmarksQueue {
    write_connection: Connection,
    read_master_connection: Connection,
}

impl SqlConstruct for SqlReplayBookmarksQueue {
    const LABEL: &'static str = "replaybookmarksqueue";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-replaybookmarksqueue.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        // NOTE: We read from master to avoid reading queue entries we just released.
        Self {
            write_connection: connections.write_connection,
            read_master_connection: connections.read_master_connection,
        }
    }
}

impl SqlReplayBookmarksQueue {
    pub async fn fetch_batch(
        &self,
        repo_name: &String,
        backfill: Backfill,
        limit: QueueLimit,
    ) -> Result<Batch, Error> {
        let rows = FetchQueue::query(
            &self.read_master_connection,
            repo_name,
            &(if backfill.0 { 1 } else { 0 }),
            &limit.0,
        )
        .compat()
        .await?;
        let mut r = HashMap::new();
        for (id, bookmark, hex_cs_id, dt) in rows.into_iter() {
            let hex_cs_id = AsciiString::from_ascii(hex_cs_id)?;
            let cs_id = HgChangesetId::from_ascii_str(&hex_cs_id)?;

            // TODO: (torozco) T46163772 queries! macro doesn't support time. MySQL Async does
            // support it, but we don't have it in SQLite, so I don't think we can expose a
            // return value for the query that works for both. For now, considering we a) only
            // use timestamps for logging and b) our local time is the same as the MySQL
            // server's time (and this all appears to work properly), Naive dates should be OK.
            let dt = NaiveDateTime::parse_from_str(&dt, DATE_TIME_FORMAT)?;

            r.entry(bookmark)
                .or_insert_with(|| BookmarkBatch {
                    dt: dt.clone(),
                    entries: vec![],
                })
                .entries
                .push((id, cs_id, dt));
        }
        Ok(r)
    }

    pub async fn release_entries(&self, entries: &[Entry]) -> Result<(), Error> {
        let ids: Vec<_> = entries.iter().map(|e| e.0).collect();
        ReleaseEntries::query(&self.write_connection, &ids[..])
            .compat()
            .await?;
        Ok(())
    }
}

#[cfg(test)]
pub mod test_helpers {
    use super::*;

    use anyhow::Result;
    use futures::compat::Future01CompatExt;
    use mercurial_types::hash::Sha1;

    pub const BACKFILL: Backfill = Backfill(true);
    pub const NOT_BACKFILL: Backfill = Backfill(false);

    pub fn t0() -> NaiveDateTime {
        NaiveDateTime::from_timestamp(10, 0)
    }

    pub fn t1() -> NaiveDateTime {
        NaiveDateTime::from_timestamp(10, 0)
    }

    pub fn batch(dt: NaiveDateTime, entries: Vec<Entry>) -> BookmarkBatch {
        BookmarkBatch { dt, entries }
    }

    queries! {
        write InsertEntries(
            values: (id: i64, repo_name: String, bookmark: String, node: String, bookmark_hash: String, created_at: String, backfill: i64)
        ) {
            none,
            "INSERT INTO replaybookmarksqueue (id, reponame, bookmark, node, bookmark_hash, created_at, backfill) VALUES {values}"
        }
    }

    pub async fn insert_entries(
        queue: &SqlReplayBookmarksQueue,
        entries: &[(i64, String, BookmarkName, Sha1, NaiveDateTime, Backfill)],
    ) -> Result<()> {
        let rows: Vec<_> = entries
            .iter()
            .map(|(id, repo, bookmark, cs_id, t, backfill)| {
                (
                    id,
                    repo,
                    bookmark.to_string(),
                    cs_id.to_string(),
                    t.format(DATE_TIME_FORMAT).to_string(),
                    (if backfill.0 { 1 } else { 0 }) as i64,
                )
            })
            .collect();

        // NOTE: we don't actually compute the bookmark_hash here because we don't use that yet.
        let refs: Vec<_> = rows
            .iter()
            .map(|(id, repo, bookmark, cs_id, t, b)| (*id, *repo, bookmark, cs_id, bookmark, t, b))
            .collect();

        InsertEntries::query(&queue.write_connection, &refs[..])
            .compat()
            .await?;

        Ok(())
    }
}

#[cfg(test)]
pub(crate) mod test {
    use super::*;
    use test_helpers::*;

    use anyhow::Result;
    use maplit::hashmap;
    use mercurial_types_mocks::{hash, nodehash};

    const QUEUE_LIMIT: QueueLimit = QueueLimit(10);

    #[tokio::test]
    async fn test_fetch_basic_batch() -> Result<()> {
        let queue = SqlReplayBookmarksQueue::with_sqlite_in_memory()?;

        let repo = "repo1".to_string();
        let book1 = BookmarkName::new("book1")?;
        let book2 = BookmarkName::new("book2")?;

        let entries = vec![
            (
                1 as i64,
                repo.clone(),
                book1.clone(),
                hash::ONES,
                t0(),
                NOT_BACKFILL,
            ),
            (
                2 as i64,
                repo.clone(),
                book2.clone(),
                hash::TWOS,
                t0(),
                NOT_BACKFILL,
            ),
        ];
        insert_entries(&queue, &entries).await?;

        let real = queue.fetch_batch(&repo, NOT_BACKFILL, QUEUE_LIMIT).await?;

        let expected = hashmap! {
            book1 => batch(t0(), vec![(1 as i64, nodehash::ONES_CSID, t0())]),
            book2 => batch(t0(), vec![(2 as i64, nodehash::TWOS_CSID, t0())]),
        };

        assert_eq!(real, expected);

        Ok(())
    }

    #[tokio::test]
    async fn test_fetch_coalesced_batch() -> Result<()> {
        let queue = SqlReplayBookmarksQueue::with_sqlite_in_memory()?;

        let repo = "repo1".to_string();
        let book1 = BookmarkName::new("book1")?;
        let book2 = BookmarkName::new("book2")?;

        let entries = vec![
            (
                1 as i64,
                repo.clone(),
                book1.clone(),
                hash::ONES,
                t0(),
                NOT_BACKFILL,
            ),
            (
                2 as i64,
                repo.clone(),
                book2.clone(),
                hash::TWOS,
                t0(),
                NOT_BACKFILL,
            ),
            (
                3 as i64,
                repo.clone(),
                book1.clone(),
                hash::THREES,
                t0(),
                NOT_BACKFILL,
            ),
            (
                4 as i64,
                repo.clone(),
                book1.clone(),
                hash::FOURS,
                t0(),
                NOT_BACKFILL,
            ),
            (
                5 as i64,
                repo.clone(),
                book1.clone(),
                hash::FIVES,
                t0(),
                NOT_BACKFILL,
            ),
        ];
        insert_entries(&queue, &entries).await?;

        let real = queue.fetch_batch(&repo, NOT_BACKFILL, QUEUE_LIMIT).await?;

        let book1_entries = vec![
            (1 as i64, nodehash::ONES_CSID, t0()),
            (3 as i64, nodehash::THREES_CSID, t0()),
            (4 as i64, nodehash::FOURS_CSID, t0()),
            (5 as i64, nodehash::FIVES_CSID, t0()),
        ];

        let expected = hashmap! {
            book1 => batch(t0(), book1_entries),
            book2 => batch(t0(), vec![(2 as i64, nodehash::TWOS_CSID, t0())]),
        };

        assert_eq!(real, expected);

        Ok(())
    }

    #[tokio::test]
    async fn test_release_batch() -> Result<()> {
        let queue = SqlReplayBookmarksQueue::with_sqlite_in_memory()?;

        let repo = "repo1".to_string();
        let book1 = BookmarkName::new("book1")?;
        let book2 = BookmarkName::new("book2")?;

        let entries = vec![
            (
                1 as i64,
                repo.clone(),
                book1.clone(),
                hash::ONES,
                t0(),
                NOT_BACKFILL,
            ),
            (
                2 as i64,
                repo.clone(),
                book2.clone(),
                hash::TWOS,
                t0(),
                NOT_BACKFILL,
            ),
            (
                3 as i64,
                repo.clone(),
                book1.clone(),
                hash::THREES,
                t0(),
                NOT_BACKFILL,
            ),
            (
                4 as i64,
                repo.clone(),
                book2.clone(),
                hash::FOURS,
                t0(),
                NOT_BACKFILL,
            ),
        ];
        insert_entries(&queue, &entries).await?;

        let release = vec![
            (1 as i64, nodehash::ONES_CSID, t0()),
            (2 as i64, nodehash::TWOS_CSID, t0()),
        ];

        queue.release_entries(&release).await?;

        let real = queue.fetch_batch(&repo, NOT_BACKFILL, QUEUE_LIMIT).await?;

        let expected = hashmap! {
            book1 => batch(t0(), vec![(3 as i64, nodehash::THREES_CSID, t0())]),
            book2 => batch(t0(), vec![(4 as i64, nodehash::FOURS_CSID, t0())]),
        };

        assert_eq!(real, expected);

        Ok(())
    }

    #[tokio::test]
    async fn test_repo_filtering() -> Result<()> {
        let queue = SqlReplayBookmarksQueue::with_sqlite_in_memory()?;

        let repo1 = "repo1".to_string();
        let repo2 = "repo2".to_string();
        let book1 = BookmarkName::new("book1")?;
        let book2 = BookmarkName::new("book2")?;

        let entries = vec![
            (
                1 as i64,
                repo1.to_string(),
                book1.clone(),
                hash::ONES,
                t0(),
                NOT_BACKFILL,
            ),
            (
                2 as i64,
                repo2.to_string(),
                book2.clone(),
                hash::TWOS,
                t0(),
                NOT_BACKFILL,
            ),
        ];
        insert_entries(&queue, &entries).await?;

        let real = queue.fetch_batch(&repo1, NOT_BACKFILL, QUEUE_LIMIT).await?;

        let expected = hashmap! {
            book1 => batch(t0(), vec![(1 as i64, nodehash::ONES_CSID, t0())]),
        };

        assert_eq!(real, expected);

        Ok(())
    }

    #[tokio::test]
    async fn test_batch_age() -> Result<()> {
        let queue = SqlReplayBookmarksQueue::with_sqlite_in_memory()?;

        let repo = "repo1".to_string();
        let book1 = BookmarkName::new("book1")?;

        let entries = vec![
            (
                1 as i64,
                repo.to_string(),
                book1.clone(),
                hash::ONES,
                t0(),
                NOT_BACKFILL,
            ),
            (
                2 as i64,
                repo.to_string(),
                book1.clone(),
                hash::TWOS,
                t1(),
                NOT_BACKFILL,
            ),
        ];
        insert_entries(&queue, &entries).await?;

        let real = queue.fetch_batch(&repo, NOT_BACKFILL, QUEUE_LIMIT).await?;

        let entries = vec![
            (1 as i64, nodehash::ONES_CSID, t0()),
            (2 as i64, nodehash::TWOS_CSID, t1()),
        ];

        let expected = hashmap! { book1 => batch(t0(), entries) };

        assert_eq!(real, expected);

        Ok(())
    }

    #[tokio::test]
    async fn test_limit() -> Result<()> {
        let queue = SqlReplayBookmarksQueue::with_sqlite_in_memory()?;

        let repo = "repo1".to_string();
        let book1 = BookmarkName::new("book1")?;
        let book2 = BookmarkName::new("book2")?;

        let entries = vec![
            (
                1 as i64,
                repo.clone(),
                book1.clone(),
                hash::ONES,
                t0(),
                NOT_BACKFILL,
            ),
            (
                2 as i64,
                repo.clone(),
                book2.clone(),
                hash::TWOS,
                t0(),
                NOT_BACKFILL,
            ),
            (
                3 as i64,
                repo.clone(),
                book1.clone(),
                hash::THREES,
                t0(),
                NOT_BACKFILL,
            ),
            (
                4 as i64,
                repo.clone(),
                book2.clone(),
                hash::FOURS,
                t0(),
                NOT_BACKFILL,
            ),
        ];
        insert_entries(&queue, &entries).await?;

        let real = queue
            .fetch_batch(&repo, NOT_BACKFILL, QueueLimit(2))
            .await?;

        let expected = hashmap! {
            book1 => batch(t0(), vec![(1 as i64, nodehash::ONES_CSID, t0())]),
            book2 => batch(t0(), vec![(2 as i64, nodehash::TWOS_CSID, t0())]),
        };

        assert_eq!(real, expected);

        Ok(())
    }

    #[tokio::test]
    async fn test_backfill() -> Result<()> {
        let queue = SqlReplayBookmarksQueue::with_sqlite_in_memory()?;

        let repo = "repo1".to_string();
        let book1 = BookmarkName::new("book1")?;

        let entries = vec![
            (
                1 as i64,
                repo.to_string(),
                book1.clone(),
                hash::ONES,
                t0(),
                NOT_BACKFILL,
            ),
            (
                2 as i64,
                repo.to_string(),
                book1.clone(),
                hash::TWOS,
                t0(),
                BACKFILL,
            ),
        ];
        insert_entries(&queue, &entries).await?;

        let real_backfill = queue.fetch_batch(&repo, BACKFILL, QUEUE_LIMIT).await?;
        let real_not_backfill = queue.fetch_batch(&repo, NOT_BACKFILL, QUEUE_LIMIT).await?;

        let expected_backfill = hashmap! {
            book1.clone() => batch(t0(), vec![(2 as i64, nodehash::TWOS_CSID, t0())])
        };

        let expected_not_backfill = hashmap! {
            book1.clone() => batch(t0(), vec![(1 as i64, nodehash::ONES_CSID, t0())])
        };

        assert_eq!(real_backfill, expected_backfill);
        assert_eq!(real_not_backfill, expected_not_backfill);

        Ok(())
    }
}
