/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error, Result};
use async_trait::async_trait;
use bookmarks::BookmarkName;
use chrono::Local;
use cloned::cloned;
use futures::stream::{self, Stream, StreamExt};
use mercurial_types::HgChangesetId;
use scuba_ext::ScubaSampleBuilder;
use slog::{info, Logger};
use stats::prelude::*;
use std::fmt::Debug;

use crate::errors::ErrorKind;
use crate::sql_replay_bookmarks_queue::{
    Backfill, BookmarkBatch, Entry, QueueLimit, RepoName, SqlReplayBookmarksQueue,
};

define_stats! {
    prefix = "mononoke.commitcloud_bookmarks_filler.replay_stream";
    batch_loaded: timeseries(Rate, Sum),
}

type History = Vec<Result<(), ErrorKind>>;

#[derive(Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
pub struct BufferSize(pub usize);

#[async_trait]
pub trait ReplayFn: Copy {
    async fn replay(
        &self,
        repo: RepoName,
        bookmark_name: BookmarkName,
        hg_cs_id: HgChangesetId,
    ) -> Result<(), ErrorKind>;
}

async fn replay_one_bookmark<'a, R: ReplayFn>(
    repo_name: RepoName,
    bookmark: BookmarkName,
    mut entries: Vec<Entry>,
    replay: R,
) -> (RepoName, BookmarkName, Result<Vec<Entry>>, History) {
    let mut history = vec![];
    while let Some((_id, hg_cs_id, _timestamp)) = entries.last() {
        let ret = replay
            .replay(repo_name.clone(), bookmark.clone(), hg_cs_id.clone())
            .await;
        let ok = ret.is_ok();
        history.push(ret);

        if ok {
            return (repo_name, bookmark, Ok(entries), history);
        }
        entries.pop();
    }
    let e = format_err!("No valid changeset to replay for bookmark: {:?}", bookmark);
    (repo_name, bookmark, Err(e), history)
}

async fn process_replay_single_batch<'a, R: ReplayFn>(
    queue: &'a SqlReplayBookmarksQueue,
    enabled_repo_names: &'a [String],
    backfill: Backfill,
    buffer_size: BufferSize,
    queue_limit: QueueLimit,
    status_scuba: ScubaSampleBuilder,
    logger: &Logger,
    replay: R,
) -> Result<(), Error> {
    let batch = queue
        .fetch_batch(enabled_repo_names, backfill, queue_limit)
        .await?;

    STATS::batch_loaded.add_value(batch.len() as i64);
    info!(logger, "Processing batch: {:?} entries", batch.len());

    stream::iter(batch.into_iter())
        .for_each_concurrent(buffer_size.0, {
            cloned!(logger, mut status_scuba, queue);
            move |((repo, bookmark), BookmarkBatch { dt, entries })| {
                cloned!(logger, mut status_scuba, queue);
                async move {
                    cloned!(queue);
                    let (repo_name, bookmark, outcome, history) =
                        replay_one_bookmark(repo, bookmark, entries, replay).await;

                    info!(
                        logger,
                        "Outcome: repo: {:?}: bookmark: {:?}: success: {:?}",
                        repo_name,
                        bookmark,
                        outcome.is_ok()
                    );

                    let latency = Local::now()
                        .naive_local()
                        .signed_duration_since(dt)
                        .num_milliseconds();

                    status_scuba
                        .add("reponame", repo_name)
                        .add("bookmark", bookmark.into_string())
                        .add("history", format!("{:?}", history))
                        .add("success", outcome.is_ok())
                        .add("sync_latency_ms", latency)
                        .log();

                    if let Ok(entries) = outcome {
                        if let Err(e) = queue.release_entries(&entries).await {
                            info!(logger, "Error while releasing queue entries: {:?}", e,);
                        }
                    };
                }
            }
        })
        .await;
    Ok(())
}

pub fn process_replay_stream<'a, R: ReplayFn + 'a>(
    queue: &'a SqlReplayBookmarksQueue,
    enabled_repo_names: &'a [String],
    backfill: Backfill,
    buffer_size: BufferSize,
    queue_limit: QueueLimit,
    status_scuba: ScubaSampleBuilder,
    logger: &'a Logger,
    replay: R,
) -> impl Stream<Item = Result<(), Error>> + 'a {
    stream::repeat(()).then({
        move |_| {
            cloned!(status_scuba);
            process_replay_single_batch(
                queue,
                enabled_repo_names,
                backfill,
                buffer_size,
                queue_limit,
                status_scuba,
                logger,
                replay,
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

    #[derive(Copy, Clone)]
    struct ReplaySuccess;
    #[async_trait]
    impl ReplayFn for ReplaySuccess {
        async fn replay(
            &self,
            _repo: RepoName,
            _name: BookmarkName,
            _cs_id: HgChangesetId,
        ) -> Result<(), ErrorKind> {
            Ok(())
        }
    }

    #[derive(Copy, Clone)]
    struct ReplayFail;
    #[async_trait]
    impl ReplayFn for ReplayFail {
        async fn replay(
            &self,
            _repo: RepoName,
            _name: BookmarkName,
            _cs_id: HgChangesetId,
        ) -> Result<(), ErrorKind> {
            Err(ErrorKind::BlobRepoError(Error::msg("err")))
        }
    }

    #[derive(Copy, Clone)]
    struct ReplayTwos;
    #[async_trait]
    impl ReplayFn for ReplayTwos {
        async fn replay(
            &self,
            _repo: RepoName,
            _name: BookmarkName,
            cs_id: HgChangesetId,
        ) -> Result<(), ErrorKind> {
            if cs_id == nodehash::TWOS_CSID {
                Ok(())
            } else {
                Err(ErrorKind::BlobRepoError(Error::msg("err")))
            }
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
        insert_entries(&queue, &entries).await?;

        process_replay_stream(
            &queue,
            vec![repo.clone()].as_slice(),
            NOT_BACKFILL,
            BUFFER_SIZE,
            QUEUE_LIMIT,
            scuba(),
            &logger(),
            ReplaySuccess,
        )
        .boxed()
        .next()
        .await
        .unwrap()?;

        let real = queue
            .fetch_batch(vec![repo.clone()].as_slice(), NOT_BACKFILL, QUEUE_LIMIT)
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
        insert_entries(&queue, &entries).await?;

        process_replay_stream(
            &queue,
            vec![repo.clone()].as_slice(),
            NOT_BACKFILL,
            BUFFER_SIZE,
            QUEUE_LIMIT,
            scuba(),
            &logger(),
            ReplayFail,
        )
        .boxed()
        .next()
        .await
        .unwrap()?;

        let expected = hashmap! {
            (repo.clone(), book1) => batch(t0(), vec![(1 as i64, nodehash::ONES_CSID, t0())]),
            (repo.clone(), book2) => batch(t0(), vec![(2 as i64, nodehash::TWOS_CSID, t0())]),
        };
        let real = queue
            .fetch_batch(vec![repo.clone()].as_slice(), NOT_BACKFILL, QUEUE_LIMIT)
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
        insert_entries(&queue, &entries).await?;

        process_replay_stream(
            &queue,
            vec![repo.clone()].as_slice(),
            NOT_BACKFILL,
            BUFFER_SIZE,
            QUEUE_LIMIT,
            scuba(),
            &logger(),
            ReplayTwos,
        )
        .boxed()
        .next()
        .await
        .unwrap()?;


        let expected = hashmap! {
            (repo.clone(), book1) => batch(t0(), vec![(3 as i64, nodehash::THREES_CSID, t0())]),
        };

        let real = queue
            .fetch_batch(vec![repo.clone()].as_slice(), NOT_BACKFILL, QUEUE_LIMIT)
            .await?;
        assert_eq!(real, expected);

        Ok(())
    }
}
