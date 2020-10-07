/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error, Result};
use bookmarks::BookmarkName;
use chrono::Local;
use cloned::cloned;
use futures::stream::{self, Stream, StreamExt};
use futures::{compat::Future01CompatExt, Future};
use mercurial_types::HgChangesetId;
use scuba_ext::ScubaSampleBuilder;
use slog::{info, Logger};
use stats::prelude::*;
use std::fmt::Debug;

use crate::errors::ErrorKind;
use crate::sql_replay_bookmarks_queue::{
    Backfill, BookmarkBatch, Entry, QueueLimit, SqlReplayBookmarksQueue,
};

define_stats! {
    prefix = "mononoke.commitcloud_bookmarks_filler.replay_stream";
    batch_loaded: timeseries(Rate, Sum),
}

type History = Vec<Result<(), ErrorKind>>;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct BufferSize(pub usize);

async fn replay_one_bookmark<F, R>(
    bookmark: BookmarkName,
    mut entries: Vec<Entry>,
    do_replay: R,
) -> (BookmarkName, Result<Vec<Entry>>, History)
where
    F: Future<Output = Result<(), ErrorKind>>,
    R: Fn(BookmarkName, HgChangesetId) -> F + Sized + Clone,
{
    let mut history = vec![];
    while let Some((_id, hg_cs_id, _timestamp)) = entries.last() {
        let ret = do_replay(bookmark.clone(), hg_cs_id.clone()).await;
        let ok = ret.is_ok();
        history.push(ret);

        if ok {
            return (bookmark, Ok(entries), history);
        }
        entries.pop();
    }
    let e = format_err!("No valid changeset to replay for bookmark: {:?}", bookmark);
    (bookmark, Err(e), history)
}

async fn process_replay_single_batch<F, R>(
    queue: &SqlReplayBookmarksQueue,
    repo_name: String,
    backfill: Backfill,
    buffer_size: BufferSize,
    queue_limit: QueueLimit,
    status_scuba: ScubaSampleBuilder,
    logger: Logger,
    do_replay: R,
) -> Result<(), Error>
where
    F: Future<Output = Result<(), ErrorKind>>,
    R: Fn(BookmarkName, HgChangesetId) -> F + Sized + Clone,
{
    let batch = queue
        .fetch_batch(&repo_name, backfill, queue_limit)
        .compat()
        .await?;

    STATS::batch_loaded.add_value(batch.len() as i64);
    info!(logger, "Processing batch: {:?} entries", batch.len());

    stream::iter(batch.into_iter())
        .for_each_concurrent(buffer_size.0, {
            cloned!(logger, mut status_scuba, queue, do_replay);
            move |(bookmark, BookmarkBatch { dt, entries })| {
                cloned!(logger, mut status_scuba, queue, do_replay);
                async move {
                    cloned!(queue, do_replay);
                    let (bookmark, outcome, history) =
                        replay_one_bookmark(bookmark, entries, do_replay).await;

                    info!(
                        logger,
                        "Outcome: bookmark: {:?}: success: {:?}",
                        bookmark,
                        outcome.is_ok()
                    );

                    let latency = Local::now()
                        .naive_local()
                        .signed_duration_since(dt)
                        .num_milliseconds();

                    status_scuba
                        .add("bookmark", bookmark.into_string())
                        .add("history", format!("{:?}", history))
                        .add("success", outcome.is_ok())
                        .add("sync_latency_ms", latency)
                        .log();

                    if let Ok(entries) = outcome {
                        if let Err(e) = queue.release_entries(&entries).compat().await {
                            info!(logger, "Error while releasing queue entries: {:?}", e,);
                        }
                    };
                }
            }
        })
        .await;
    Ok(())
}

pub fn process_replay_stream<'a, F, R>(
    queue: &'a SqlReplayBookmarksQueue,
    repo_name: String,
    backfill: Backfill,
    buffer_size: BufferSize,
    queue_limit: QueueLimit,
    status_scuba: ScubaSampleBuilder,
    logger: Logger,
    do_replay: R,
) -> impl Stream<Item = Result<(), Error>> + 'a
where
    F: Future<Output = Result<(), ErrorKind>> + 'a,
    R: Fn(BookmarkName, HgChangesetId) -> F + Sized + Clone + 'a,
{
    stream::repeat(()).then({
        move |_| {
            cloned!(logger, status_scuba, repo_name, do_replay);
            process_replay_single_batch(
                queue,
                repo_name,
                backfill,
                buffer_size,
                queue_limit,
                status_scuba,
                logger,
                do_replay,
            )
        }
    })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::sql_replay_bookmarks_queue::test_helpers::*;

    use maplit::hashmap;
    use mercurial_types_mocks::{hash, nodehash};
    use sql_construct::SqlConstruct;

    const BUFFER_SIZE: BufferSize = BufferSize(10);
    const QUEUE_LIMIT: QueueLimit = QueueLimit(10);

    async fn replay_success(_name: BookmarkName, _cs_id: HgChangesetId) -> Result<(), ErrorKind> {
        Ok(())
    }

    async fn replay_fail(_name: BookmarkName, _cs_id: HgChangesetId) -> Result<(), ErrorKind> {
        Err(ErrorKind::BlobRepoError(Error::msg("err")))
    }

    async fn replay_twos(_name: BookmarkName, cs_id: HgChangesetId) -> Result<(), ErrorKind> {
        if cs_id == nodehash::TWOS_CSID {
            Ok(())
        } else {
            Err(ErrorKind::BlobRepoError(Error::msg("err")))
        }
    }

    fn scuba() -> ScubaSampleBuilder {
        ScubaSampleBuilder::with_discard()
    }

    fn logger() -> Logger {
        Logger::root(slog::Discard, slog::o!())
    }

    #[tokio::test]
    async fn test_sync_success() -> Result<()> {
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
        insert_entries_async(&queue, &entries).await?;

        process_replay_stream(
            &queue,
            repo.clone(),
            NOT_BACKFILL,
            BUFFER_SIZE,
            QUEUE_LIMIT,
            scuba(),
            logger(),
            replay_success,
        )
        .boxed()
        .next()
        .await
        .unwrap()?;

        let real = queue
            .fetch_batch(&repo, NOT_BACKFILL, QUEUE_LIMIT)
            .compat()
            .await?;
        assert_eq!(real, hashmap! {});

        Ok(())
    }

    #[tokio::test]
    async fn test_sync_fail() -> Result<()> {
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
        insert_entries_async(&queue, &entries).await?;

        process_replay_stream(
            &queue,
            repo.clone(),
            NOT_BACKFILL,
            BUFFER_SIZE,
            QUEUE_LIMIT,
            scuba(),
            logger(),
            replay_fail,
        )
        .boxed()
        .next()
        .await
        .unwrap()?;

        let expected = hashmap! {
            book1 => batch(t0(), vec![(1 as i64, nodehash::ONES_CSID, t0())]),
            book2 => batch(t0(), vec![(2 as i64, nodehash::TWOS_CSID, t0())]),
        };
        let real = queue
            .fetch_batch(&repo, NOT_BACKFILL, QUEUE_LIMIT)
            .compat()
            .await?;
        assert_eq!(real, expected);

        Ok(())
    }

    #[tokio::test]
    async fn test_sync_partial() -> Result<()> {
        let queue = SqlReplayBookmarksQueue::with_sqlite_in_memory()?;

        let repo = "repo1".to_string();
        let book1 = BookmarkName::new("book1")?;

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
                book1.clone(),
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
        ];
        insert_entries_async(&queue, &entries).await?;

        process_replay_stream(
            &queue,
            repo.clone(),
            NOT_BACKFILL,
            BUFFER_SIZE,
            QUEUE_LIMIT,
            scuba(),
            logger(),
            replay_twos,
        )
        .boxed()
        .next()
        .await
        .unwrap()?;


        let expected = hashmap! {
            book1 => batch(t0(), vec![(3 as i64, nodehash::THREES_CSID, t0())]),
        };

        let real = queue
            .fetch_batch(&repo, NOT_BACKFILL, QUEUE_LIMIT)
            .compat()
            .await?;
        assert_eq!(real, expected);

        Ok(())
    }
}
