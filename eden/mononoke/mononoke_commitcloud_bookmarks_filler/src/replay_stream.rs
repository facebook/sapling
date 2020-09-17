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
use futures_ext::FutureExt;
use futures_old::{
    future::{self, Future, Loop},
    stream::{self, repeat, Stream},
};
use mercurial_types::HgChangesetId;
use scuba_ext::ScubaSampleBuilder;
use slog::{info, Logger};
use stats::prelude::*;
use std::fmt::Debug;
use std::sync::Arc;

use crate::errors::ErrorKind;
use crate::sql_replay_bookmarks_queue::{
    Backfill, BookmarkBatch, Entry, QueueLimit, SqlReplayBookmarksQueue,
};

define_stats! {
    prefix = "mononoke.commitcloud_bookmarks_filler.replay_stream";
    batch_loaded: timeseries(Rate, Sum),
}

type History<O> = Vec<Result<O, ErrorKind>>;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct BufferSize(pub usize);

fn replay_one_bookmark<F, R, O>(
    bookmark: BookmarkName,
    entries: Vec<Entry>,
    do_replay: R,
) -> impl Future<Item = (BookmarkName, Result<Vec<Entry>>, History<O>), Error = !>
where
    F: Future<Item = O, Error = ErrorKind> + 'static,
    R: Fn(&BookmarkName, &HgChangesetId) -> F + Sized,
    O: Sized,
{
    // Our loop state is a 3-tuple consisting of:
    // - The bookmark we are syncing
    // - The entries left to sync
    // - The history of outcomes (Results)
    future::loop_fn(
        (bookmark, entries, vec![]),
        move |(bookmark, mut entries, mut history)| match entries.last() {
            // If we exhausted the entries, then give up.
            None => {
                let e = format_err!("No valid changeset to replay for bookmark: {:?}", bookmark);
                future::err((bookmark, e, history))
            }
            .left_future(),
            // If we still have entries, then try to sync the last element (i.e. the most recent
            // move). If we succeed, then return this as the replayed entries. If not, then pop the
            // element we just synced, and try again.
            Some((_id, hg_cs_id, _timestamp)) => {
                do_replay(&bookmark, hg_cs_id).then(move |r| {
                    let ok = r.is_ok();
                    history.push(r);

                    let next = if ok {
                        Loop::Break((bookmark, entries, history))
                    } else {
                        entries.pop();
                        Loop::Continue((bookmark, entries, history))
                    };

                    future::ok(next)
                })
            }
            .right_future(),
        },
    )
    .then(|r| match r {
        Ok((bookmark, synced, history)) => Ok((bookmark, Ok(synced), history)),
        Err((bookmark, error, history)) => Ok((bookmark, Err(error), history)),
    })
}

pub fn process_replay_stream<F, R, O>(
    queue: SqlReplayBookmarksQueue,
    repo_name: String,
    backfill: Backfill,
    buffer_size: BufferSize,
    queue_limit: QueueLimit,
    status_scuba: ScubaSampleBuilder,
    logger: Logger,
    do_replay: R,
) -> impl Stream<Item = (), Error = Error>
where
    F: Future<Item = O, Error = ErrorKind> + 'static,
    R: Fn(&BookmarkName, &HgChangesetId) -> F + Sized + Clone,
    O: Sized + Debug,
{
    let queue = Arc::new(queue);

    repeat(())
        .and_then({
            cloned!(queue);
            move |_| queue.fetch_batch(&repo_name, backfill, queue_limit)
        })
        .and_then({
            move |batch| {
                let futs: Vec<_> = batch
                    .into_iter()
                    .map(|(bookmark, BookmarkBatch { dt, entries })| {
                        replay_one_bookmark(bookmark, entries, do_replay.clone())
                            .map(move |res| (dt, res))
                    })
                    .collect();

                STATS::batch_loaded.add_value(futs.len() as i64);
                info!(logger, "Processing batch: {:?} entries", futs.len());

                stream::iter_ok(futs)
                    .buffer_unordered(buffer_size.0)
                    .map({
                        cloned!(logger, mut status_scuba, queue);
                        move |(dt, (bookmark, outcome, history))| {
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

                            let fut = match outcome {
                                Ok(entries) => queue.release_entries(&entries).left_future(),
                                Err(_reason) => future::ok(()).right_future(),
                            };

                            fut
                        }
                    })
                    .from_err()
                    .buffer_unordered(buffer_size.0)
                    .for_each(|_| Ok(()))
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
    use tokio_compat::runtime::Runtime;

    const BUFFER_SIZE: BufferSize = BufferSize(10);
    const QUEUE_LIMIT: QueueLimit = QueueLimit(10);

    fn replay_success(
        _name: &BookmarkName,
        _cs_id: &HgChangesetId,
    ) -> impl Future<Item = (), Error = ErrorKind> {
        future::ok(())
    }

    fn replay_fail(
        _name: &BookmarkName,
        _cs_id: &HgChangesetId,
    ) -> impl Future<Item = (), Error = ErrorKind> {
        future::err(ErrorKind::BlobRepoError(Error::msg("err")))
    }

    fn replay_twos(
        _name: &BookmarkName,
        cs_id: &HgChangesetId,
    ) -> impl Future<Item = (), Error = ErrorKind> {
        if cs_id == &nodehash::TWOS_CSID {
            future::ok(())
        } else {
            future::err(ErrorKind::BlobRepoError(Error::msg("err")))
        }
    }

    fn scuba() -> ScubaSampleBuilder {
        ScubaSampleBuilder::with_discard()
    }

    fn logger() -> Logger {
        Logger::root(slog::Discard, slog::o!())
    }

    #[test]
    fn test_sync_success() -> Result<()> {
        let mut rt = Runtime::new()?;
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
        insert_entries(&mut rt, &queue, &entries)?;

        let process = process_replay_stream(
            queue.clone(),
            repo.clone(),
            NOT_BACKFILL,
            BUFFER_SIZE,
            QUEUE_LIMIT,
            scuba(),
            logger(),
            replay_success,
        )
        .take(1)
        .collect();

        rt.block_on(process)?;

        let real = rt.block_on(queue.fetch_batch(&repo, NOT_BACKFILL, QUEUE_LIMIT))?;
        assert_eq!(real, hashmap! {});

        Ok(())
    }

    #[test]
    fn test_sync_fail() -> Result<()> {
        let mut rt = Runtime::new()?;
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
        insert_entries(&mut rt, &queue, &entries)?;

        let process = process_replay_stream(
            queue.clone(),
            repo.clone(),
            NOT_BACKFILL,
            BUFFER_SIZE,
            QUEUE_LIMIT,
            scuba(),
            logger(),
            replay_fail,
        )
        .take(1)
        .collect();

        rt.block_on(process)?;

        let expected = hashmap! {
            book1 => batch(t0(), vec![(1 as i64, nodehash::ONES_CSID, t0())]),
            book2 => batch(t0(), vec![(2 as i64, nodehash::TWOS_CSID, t0())]),
        };
        let real = rt.block_on(queue.fetch_batch(&repo, NOT_BACKFILL, QUEUE_LIMIT))?;
        assert_eq!(real, expected);

        Ok(())
    }

    #[test]
    fn test_sync_partial() -> Result<()> {
        let mut rt = Runtime::new()?;
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
        insert_entries(&mut rt, &queue, &entries)?;

        let process = process_replay_stream(
            queue.clone(),
            repo.clone(),
            NOT_BACKFILL,
            BUFFER_SIZE,
            QUEUE_LIMIT,
            scuba(),
            logger(),
            replay_twos,
        )
        .take(1)
        .collect();

        rt.block_on(process)?;

        let expected = hashmap! {
            book1 => batch(t0(), vec![(3 as i64, nodehash::THREES_CSID, t0())]),
        };

        let real = rt.block_on(queue.fetch_batch(&repo, NOT_BACKFILL, QUEUE_LIMIT))?;
        assert_eq!(real, expected);

        Ok(())
    }
}
