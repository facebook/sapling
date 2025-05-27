/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::time::Duration;
use std::time::Instant;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use async_trait::async_trait;
use bookmarks::BookmarkCategory;
use bookmarks::BookmarkKey;
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
    bookmarks: HashMap<BookmarkKey, (ChangesetId, BookmarkKind)>,
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

        let (txn, log_id_rows) = GetLargestLogId::maybe_traced_query_with_transaction(
            txn,
            ctx.client_request_info(),
            &sql_bookmarks.repo_id,
        )
        .await
        .context("Failed to read log id")?;

        // Our ids start at 1 so we can default log_id to zero if it's missing.
        let log_id = log_id_rows
            .into_iter()
            .map(|r| r.0)
            .next()
            .flatten()
            .unwrap_or(0);

        let tok: i32 = rand::thread_rng().r#gen();
        let (txn, bookmarks) = SelectAllUnordered::maybe_traced_query_with_transaction(
            txn,
            ctx.client_request_info(),
            &sql_bookmarks.repo_id,
            &u64::MAX,
            &tok,
            BookmarkKind::ALL_PUBLISHING,
            BookmarkCategory::ALL,
        )
        .await
        .context("Failed to query bookmarks")?;

        // Cleanly close this transaction. No need to commit: we're not making any changes here.
        txn.rollback()
            .await
            .context("Failed to close transaction")?;

        let bookmarks = bookmarks
            .into_iter()
            .map(|(name, category, kind, cs_id, _log_id, _tok)| {
                (
                    BookmarkKey::with_name_and_category(name, category),
                    (cs_id, kind),
                )
            })
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
        self.last_refresh.elapsed() > Duration::from_millis(DEFAULT_SUBSCRIPTION_MAX_AGE_MS)
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

        let changes = SelectUpdatedBookmarks::maybe_traced_query(
            conn,
            ctx.client_request_info(),
            &self.sql_bookmarks.repo_id,
            &self.log_id,
        )
        .await
        .with_context(|| format!("Failed to select updated bookmarks after {}", self.log_id))?;

        let mut max_log_id = None;
        let mut updates = HashMap::new();

        for (log_id, name, category, kind, cs_id) in changes {
            let bookmark = BookmarkKey::with_name_and_category(name, category);
            // kind & cs_id come from the same table (bookmarks) and they're not nullable there, so
            // if one is missing, that means the join didn't find anything, and the one must be
            // missing too.
            let value = match (kind, cs_id) {
                (Some(kind), Some(cs_id)) => Some((cs_id, kind)),
                (None, None) => None,
                _ => bail!("Inconsistent data for bookmark: {}", bookmark),
            };

            max_log_id = std::cmp::max(max_log_id, Some(log_id));

            // NOTE: We get the updates in DESC-ending order, so we'll always find the current
            // bookmark state first.
            updates.entry(bookmark).or_insert(value);
        }

        for (book, maybe_value) in updates.into_iter() {
            match maybe_value {
                Some(value) => {
                    self.bookmarks.insert(book, value);
                }
                None => {
                    if book.as_str() == "master" {
                        warn!(
                            ctx.logger(),
                            "BookmarksSubscription: protect master kicked in!"
                        );
                        STATS::master_protected.add_value(1);
                        continue;
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

    fn bookmarks(&self) -> &HashMap<BookmarkKey, (ChangesetId, BookmarkKind)> {
        &self.bookmarks
    }
}

mononoke_queries! {
    read SelectUpdatedBookmarks(
        repo_id: RepositoryId,
        log_id: u64
    ) -> (u64, BookmarkName, BookmarkCategory, Option<BookmarkKind>, Option<ChangesetId>) {
        mysql("
        SELECT bookmarks_update_log.id, bookmarks_update_log.name, bookmarks_update_log.category, bookmarks.hg_kind, bookmarks.changeset_id
        FROM bookmarks_update_log
        LEFT JOIN bookmarks
            FORCE INDEX (repo_id_log_id)
            ON  bookmarks.repo_id = bookmarks_update_log.repo_id
            AND bookmarks.log_id  = bookmarks_update_log.id
        WHERE bookmarks_update_log.repo_id = {repo_id} AND bookmarks_update_log.id > {log_id}
        ORDER BY bookmarks_update_log.id DESC
        ")
        sqlite("
        SELECT bookmarks_update_log.id, bookmarks_update_log.name, bookmarks_update_log.category, bookmarks.hg_kind, bookmarks.changeset_id
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
    use mononoke_macros::mononoke;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::repo::REPO_ZERO;
    use sql_construct::SqlConstruct;

    use super::*;
    use crate::builder::SqlBookmarksBuilder;

    #[mononoke::fbinit_test]
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
        let book = BookmarkKey::new("book")?;
        let rows = vec![(
            &bookmarks.repo_id,
            &book,
            &ONES_CSID,
            &BookmarkKind::Publishing,
        )];
        crate::transaction::insert_bookmarks(&conn, rows).await?;

        sub.refresh(&ctx).await?;
        assert_eq!(*sub.bookmarks(), hashmap! {});

        sub.last_refresh = Instant::now()
            .checked_sub(Duration::from_millis(DEFAULT_SUBSCRIPTION_MAX_AGE_MS + 1))
            .ok_or(anyhow::anyhow!("Invalid duration subtraction"))?;

        sub.refresh(&ctx).await?;
        assert_eq!(
            *sub.bookmarks(),
            hashmap! { book => (ONES_CSID, BookmarkKind::Publishing)}
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_protect_master(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let bookmarks = SqlBookmarksBuilder::with_sqlite_in_memory()?.with_repo_id(REPO_ZERO);
        let conn = bookmarks.connections.write_connection.clone();

        let book = BookmarkKey::new("master")?;

        let rows = vec![(
            &bookmarks.repo_id,
            &book,
            &ONES_CSID,
            &BookmarkKind::Publishing,
        )];
        crate::transaction::insert_bookmarks(&conn, rows).await?;

        let mut sub =
            SqlBookmarksSubscription::create(&ctx, bookmarks.clone(), Freshness::MostRecent)
                .await?;

        let mut txn = bookmarks.create_transaction(ctx.clone());
        txn.force_delete(&book, BookmarkUpdateReason::TestMove)?;
        txn.commit().await?;

        sub.refresh(&ctx).await?;
        assert!(sub.bookmarks.contains_key(&book));

        Ok(())
    }
}
