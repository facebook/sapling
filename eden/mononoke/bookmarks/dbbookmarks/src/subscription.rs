/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use bookmarks::BookmarksSubscription;
use bookmarks::Freshness;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use rand::Rng;
use slog::warn;
use sql_ext::mononoke_queries;
use stats::prelude::*;
use tunables::tunables;

use crate::store::GetLargestLogId;
use crate::store::SelectAllUnordered;
use crate::store::SqlBookmarks;

define_stats! {
    prefix = "mononoke.dbbookmarks.subscription";
    aged_out: timeseries(Sum),
    master_protected: timeseries(Sum),
}

const DEFAULT_SUBSCRIPTION_MAX_AGE_MS: u64 = 600_000; // 10 minutes.

#[derive(Clone)]
pub struct SqlBookmarksSubscription {
    sql_bookmarks: SqlBookmarks,
    freshness: Freshness,
    log_id: u64,
    bookmarks: HashMap<BookmarkName, (ChangesetId, BookmarkKind)>,
    last_refresh: Instant,
}

impl SqlBookmarksSubscription {
    pub async fn create(
        ctx: &CoreContext,
        sql_bookmarks: SqlBookmarks,
        freshness: Freshness,
    ) -> Result<Self> {
        // We assume that we're using the default transaction isolation levels (which is repeatable
        // read). This means that the first select here will establish a snapshot, and we'll see
        // bookmarks updated prior to this log id in the second query.

        let conn = sql_bookmarks.connection(ctx, freshness);
        let txn = conn
            .start_transaction()
            .await
            .context("Failed to start bookmarks read transaction")?;

        let (txn, log_id_rows) =
            GetLargestLogId::query_with_transaction(txn, &sql_bookmarks.repo_id)
                .await
                .context("Failed to read log id")?;

        // Our ids start at 1 so we can default log_id to zero if it's missing.
        let log_id = log_id_rows
            .into_iter()
            .map(|r| r.0)
            .next()
            .flatten()
            .unwrap_or(0);

        let tok: i32 = rand::thread_rng().gen();
        let (txn, bookmarks) = SelectAllUnordered::query_with_transaction(
            txn,
            &sql_bookmarks.repo_id,
            &std::u64::MAX,
            &tok,
            BookmarkKind::ALL_PUBLISHING,
        )
        .await
        .context("Failed to query bookmarks")?;

        // Cleanly close this transaction. No need to commit: we're not making any changes here.
        txn.rollback()
            .await
            .context("Failed to close transaction")?;

        let bookmarks = bookmarks
            .into_iter()
            .map(|(name, kind, cs_id, _log_id, _tok)| (name, (cs_id, kind)))
            .collect();

        Ok(Self {
            sql_bookmarks,
            freshness,
            log_id,
            bookmarks,
            last_refresh: Instant::now(),
        })
    }

    fn has_aged_out(&self) -> bool {
        let max_age_ms = match tunables().get_bookmark_subscription_max_age_ms().try_into() {
            Ok(duration) if duration > 0 => duration,
            _ => DEFAULT_SUBSCRIPTION_MAX_AGE_MS,
        };

        self.last_refresh.elapsed() > Duration::from_millis(max_age_ms)
    }
}

#[async_trait]
impl BookmarksSubscription for SqlBookmarksSubscription {
    async fn refresh(&mut self, ctx: &CoreContext) -> Result<()> {
        if self.has_aged_out() {
            warn!(
                ctx.logger(),
                "BookmarksSubscription has aged out! Last refresh was {:?} ago.",
                self.last_refresh.elapsed()
            );

            STATS::aged_out.add_value(1);

            *self = Self::create(ctx, self.sql_bookmarks.clone(), self.freshness)
                .await
                .context("Failed to re-initialize expired BookmarksSubscription")?;
            return Ok(());
        }

        let conn = self.sql_bookmarks.connection(ctx, self.freshness);

        let changes =
            SelectUpdatedBookmarks::query(conn, &self.sql_bookmarks.repo_id, &self.log_id)
                .await
                .with_context(|| {
                    format!("Failed to select updated bookmarks after {}", self.log_id)
                })?;

        let mut max_log_id = None;
        let mut updates = HashMap::new();

        for (log_id, name, kind, cs_id) in changes {
            // kind & cs_id come from the same table (bookmarks) and they're not nullable there, so
            // if one is missing, that means the join didn't find anything, and the one must be
            // missing too.
            let value = match (kind, cs_id) {
                (Some(kind), Some(cs_id)) => Some((cs_id, kind)),
                (None, None) => None,
                _ => bail!("Inconsistent data for bookmark: {}", name),
            };

            max_log_id = std::cmp::max(max_log_id, Some(log_id));

            // NOTE: We get the updates in DESC-ending order, so we'll always find the curent
            // bookmark state first.
            updates.entry(name).or_insert(value);
        }

        for (book, maybe_value) in updates.into_iter() {
            match maybe_value {
                Some(value) => {
                    self.bookmarks.insert(book, value);
                }
                None => {
                    if tunables().get_bookmark_subscription_protect_master() {
                        if book.as_str() == "master" {
                            warn!(
                                ctx.logger(),
                                "BookmarksSubscription: protect master kicked in!"
                            );
                            STATS::master_protected.add_value(1);
                            continue;
                        }
                    }

                    self.bookmarks.remove(&book);
                }
            }
        }

        if let Some(max_log_id) = max_log_id {
            self.log_id = max_log_id;
        }

        self.last_refresh = Instant::now();

        Ok(())
    }

    fn bookmarks(&self) -> &HashMap<BookmarkName, (ChangesetId, BookmarkKind)> {
        &self.bookmarks
    }
}

mononoke_queries! {
    read SelectUpdatedBookmarks(
        repo_id: RepositoryId,
        log_id: u64
    ) -> (u64, BookmarkName, Option<BookmarkKind>, Option<ChangesetId>) {
        mysql("
        SELECT bookmarks_update_log.id, bookmarks_update_log.name, bookmarks.hg_kind, bookmarks.changeset_id
        FROM bookmarks_update_log
        LEFT JOIN bookmarks
            FORCE INDEX (repo_id_log_id)
            ON  bookmarks.repo_id = bookmarks_update_log.repo_id
            AND bookmarks.log_id  = bookmarks_update_log.id
        WHERE bookmarks_update_log.repo_id = {repo_id} AND bookmarks_update_log.id > {log_id}
        ORDER BY bookmarks_update_log.id DESC
        ")
        sqlite("
        SELECT bookmarks_update_log.id, bookmarks_update_log.name, bookmarks.hg_kind, bookmarks.changeset_id
        FROM bookmarks_update_log
        LEFT JOIN bookmarks
            ON  bookmarks.repo_id = bookmarks_update_log.repo_id
            AND bookmarks.log_id  = bookmarks_update_log.id
        WHERE bookmarks_update_log.repo_id = {repo_id} AND bookmarks_update_log.id > {log_id}
        ORDER BY bookmarks_update_log.id DESC
        ")
    }
}

#[cfg(test)]
mod test {
    use bookmarks::BookmarkUpdateReason;
    use bookmarks::Bookmarks;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::repo::REPO_ZERO;
    use sql_construct::SqlConstruct;
    use tunables::MononokeTunables;

    use super::*;
    use crate::builder::SqlBookmarksBuilder;

    #[fbinit::test]
    async fn test_age_out(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()?.with_repo_id(REPO_ZERO);
        let conn = bookmarks.connections.write_connection.clone();

        let mut sub =
            SqlBookmarksSubscription::create(&ctx, bookmarks.clone(), Freshness::MostRecent)
                .await?;

        // Insert a bookmark, but without going throguh the log. This won't happen in prod.
        // However, for the purposes of this test, it makes the update invisible to the
        // subscription, and only a full refresh will find it.
        let book = BookmarkName::new("book")?;
        let rows = vec![(
            &bookmarks.repo_id,
            &book,
            &ONES_CSID,
            &BookmarkKind::Publishing,
        )];
        crate::transaction::insert_bookmarks(&conn, rows).await?;

        sub.refresh(&ctx).await?;
        assert_eq!(*sub.bookmarks(), hashmap! {});

        sub.last_refresh =
            Instant::now() - Duration::from_millis(DEFAULT_SUBSCRIPTION_MAX_AGE_MS + 1);

        sub.refresh(&ctx).await?;
        assert_eq!(
            *sub.bookmarks(),
            hashmap! { book => (ONES_CSID, BookmarkKind::Publishing)}
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_protect_master(fb: FacebookInit) -> Result<()> {
        let protect_master = MononokeTunables::default();
        protect_master
            .update_bools(&hashmap! {"bookmark_subscription_protect_master".to_string() => true});

        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()?.with_repo_id(REPO_ZERO);
        let conn = bookmarks.connections.write_connection.clone();

        let book = BookmarkName::new("master")?;

        let rows = vec![(
            &bookmarks.repo_id,
            &book,
            &ONES_CSID,
            &BookmarkKind::Publishing,
        )];
        crate::transaction::insert_bookmarks(&conn, rows).await?;

        let mut sub1 =
            SqlBookmarksSubscription::create(&ctx, bookmarks.clone(), Freshness::MostRecent)
                .await?;

        let mut sub2 =
            SqlBookmarksSubscription::create(&ctx, bookmarks.clone(), Freshness::MostRecent)
                .await?;

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_delete(&book, BookmarkUpdateReason::TestMove)?;
        txn.commit().await?;

        tunables::with_tunables_async(protect_master, sub1.refresh(&ctx)).await?;
        assert!(sub1.bookmarks.get(&book).is_some());

        sub2.refresh(&ctx).await?;
        assert!(sub2.bookmarks.get(&book).is_none());

        Ok(())
    }
}
