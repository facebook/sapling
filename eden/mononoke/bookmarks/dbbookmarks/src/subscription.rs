/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use bookmarks::{BookmarkKind, BookmarkName, BookmarksSubscription, Freshness};
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use rand::Rng;
use sql::queries;
use std::collections::HashMap;

use crate::store::{GetLargestLogId, SelectAllUnordered, SqlBookmarks};

#[derive(Clone)]
pub struct SqlBookmarksSubscription {
    sql_bookmarks: SqlBookmarks,
    freshness: Freshness,
    log_id: u64,
    bookmarks: HashMap<BookmarkName, (ChangesetId, BookmarkKind)>,
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
            &BookmarkKind::ALL_PUBLISHING,
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
        })
    }
}

#[async_trait]
impl BookmarksSubscription for SqlBookmarksSubscription {
    async fn refresh(&mut self, ctx: &CoreContext) -> Result<()> {
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
                    self.bookmarks.remove(&book);
                }
            }
        }

        if let Some(max_log_id) = max_log_id {
            self.log_id = max_log_id;
        }

        Ok(())
    }

    fn bookmarks(&self) -> &HashMap<BookmarkName, (ChangesetId, BookmarkKind)> {
        &self.bookmarks
    }
}

queries! {
    read SelectUpdatedBookmarks(
        repo_id: RepositoryId,
        log_id: u64
    ) -> (u64, BookmarkName, Option<BookmarkKind>, Option<ChangesetId>) {
        "
        SELECT bookmarks_update_log.id, bookmarks_update_log.name, bookmarks.hg_kind, bookmarks.changeset_id
        FROM bookmarks_update_log
        LEFT JOIN bookmarks
            ON  bookmarks.repo_id = bookmarks_update_log.repo_id
            AND bookmarks.log_id  = bookmarks_update_log.id
        WHERE bookmarks_update_log.repo_id = {repo_id} AND bookmarks_update_log.id > {log_id}
        ORDER BY bookmarks_update_log.id DESC
        "
    }
}
