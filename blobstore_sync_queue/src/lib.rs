// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use cloned::cloned;
use context::CoreContext;
use failure_ext::{err_msg, format_err, Error};
use futures::sync::{mpsc, oneshot};
use futures::{future, Future, IntoFuture, Stream};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt, StreamExt};
use metaconfig_types::BlobstoreId;
use mononoke_types::{DateTime, RepositoryId, Timestamp};
use sql::{queries, Connection};
pub use sql_ext::SqlConstructors;
use stats::{define_stats, Timeseries};
use std::sync::Arc;

define_stats! {
    prefix = "mononoke.blobstore_sync_queue";
    adds: timeseries(RATE, SUM),
    iters: timeseries(RATE, SUM),
    dels: timeseries(RATE, SUM),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BlobstoreSyncQueueEntry {
    pub repo_id: RepositoryId,
    pub blobstore_key: String,
    pub blobstore_id: BlobstoreId,
    pub timestamp: DateTime,
    pub id: Option<u64>,
}

impl BlobstoreSyncQueueEntry {
    pub fn new(
        repo_id: RepositoryId,
        blobstore_key: String,
        blobstore_id: BlobstoreId,
        timestamp: DateTime,
    ) -> Self {
        Self {
            repo_id,
            blobstore_key,
            blobstore_id,
            timestamp,
            id: None,
        }
    }
}

pub trait BlobstoreSyncQueue: Send + Sync {
    fn add(&self, ctx: CoreContext, entry: BlobstoreSyncQueueEntry) -> BoxFuture<(), Error>;

    /// Returns list of entries that consist of two groups of entries:
    /// 1. Group with at most `limit` entries that are older than `older_than`
    /// 2. Group of entries whose `blobstore_key` can be found in group (1)
    ///
    /// As a result the caller gets a reasonably limited slice of BlobstoreSyncQueue entries that
    /// are all related, so that the caller doesn't need to fetch more data from BlobstoreSyncQueue
    /// to process the sync queue.
    fn iter(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        older_than: DateTime,
        limit: usize,
    ) -> BoxFuture<Vec<BlobstoreSyncQueueEntry>, Error>;

    fn del(&self, ctx: CoreContext, entries: Vec<BlobstoreSyncQueueEntry>) -> BoxFuture<(), Error>;

    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        key: String,
    ) -> BoxFuture<Vec<BlobstoreSyncQueueEntry>, Error>;
}

impl BlobstoreSyncQueue for Arc<BlobstoreSyncQueue> {
    fn add(&self, ctx: CoreContext, entry: BlobstoreSyncQueueEntry) -> BoxFuture<(), Error> {
        (**self).add(ctx, entry)
    }

    fn iter(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        older_than: DateTime,
        limit: usize,
    ) -> BoxFuture<Vec<BlobstoreSyncQueueEntry>, Error> {
        (**self).iter(ctx, repo_id, older_than, limit)
    }

    fn del(&self, ctx: CoreContext, entries: Vec<BlobstoreSyncQueueEntry>) -> BoxFuture<(), Error> {
        (**self).del(ctx, entries)
    }

    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        key: String,
    ) -> BoxFuture<Vec<BlobstoreSyncQueueEntry>, Error> {
        (**self).get(ctx, repo_id, key)
    }
}

#[derive(Clone)]
pub struct SqlBlobstoreSyncQueue {
    write_connection: Arc<Connection>,
    read_connection: Connection,
    read_master_connection: Connection,
    write_sender:
        Arc<mpsc::UnboundedSender<(oneshot::Sender<Result<(), Error>>, BlobstoreSyncQueueEntry)>>,
    ensure_worker_scheduled: future::Shared<BoxFuture<(), ()>>,
}

queries! {
    write InsertEntry(values: (
        repo_id: RepositoryId,
        blobstore_key: String,
        blobstore_id: BlobstoreId,
        timestamp: Timestamp,
    )) {
        insert_or_ignore,
        "{insert_or_ignore}
         INTO blobstore_sync_queue (repo_id, blobstore_key, blobstore_id, add_timestamp)
         VALUES {values}"
    }

    write DeleteEntry(id: u64) {
        none,
        "DELETE FROM blobstore_sync_queue
         WHERE id = {id}"
    }

    read GetAllIEntries() -> (RepositoryId, String, BlobstoreId, Timestamp, u64) {
        "SELECT repo_id, blobstore_key, blobstore_id, add_timestamp, id
         FROM blobstore_sync_queue"
    }

    read GetRangeOfEntries(repo_id: RepositoryId, older_than: Timestamp, limit: usize) -> (
        RepositoryId,
        String,
        BlobstoreId,
        Timestamp,
        u64,
    ) {
        "SELECT repo_id, blobstore_sync_queue.blobstore_key, blobstore_id, add_timestamp, id
         FROM blobstore_sync_queue
         JOIN (
               SELECT DISTINCT blobstore_key
               FROM blobstore_sync_queue
               WHERE repo_id = {repo_id}
                 AND add_timestamp <= {older_than}
               LIMIT {limit}
         ) b
         ON blobstore_sync_queue.blobstore_key = b.blobstore_key
         WHERE repo_id = {repo_id}"
    }

    read GetByKey(repo_id: RepositoryId, key: String) -> (
        RepositoryId,
        String,
        BlobstoreId,
        Timestamp,
        u64,
    ) {
        "SELECT repo_id, blobstore_key, blobstore_id, add_timestamp, id
         FROM blobstore_sync_queue
         WHERE repo_id = {repo_id}
         AND blobstore_key = {key}"
    }
}

impl SqlConstructors for SqlBlobstoreSyncQueue {
    const LABEL: &'static str = "blobstore_sync_queue";

    fn from_connections(
        write_connection: Connection,
        read_connection: Connection,
        read_master_connection: Connection,
    ) -> Self {
        let write_connection = Arc::new(write_connection);
        type ChannelType = (oneshot::Sender<Result<(), Error>>, BlobstoreSyncQueueEntry);
        let (sender, receiver): (mpsc::UnboundedSender<ChannelType>, _) = mpsc::unbounded();

        let ensure_worker_scheduled = future::lazy({
            cloned!(write_connection);
            move || {
                let batch_writes = receiver.batch(WRITE_BUFFER_SIZE).for_each({
                    cloned!(write_connection);
                    move |batch| {
                        let (senders, entries): (Vec<_>, Vec<_>) = batch.into_iter().unzip();

                        insert_entries(write_connection.clone(), entries).then(move |res| {
                            match res {
                                Ok(()) => {
                                    for sender in senders {
                                        // Ignoring the error, because receiver might have gone
                                        let _ = sender.send(Ok(()));
                                    }
                                }
                                Err(err) => {
                                    let s = format!("failed to insert {}", err);
                                    for sender in senders {
                                        // Ignoring the error, because receiver might have gone
                                        let _ = sender.send(Err(err_msg(s.clone())));
                                    }
                                }
                            }
                            Ok(())
                        })
                    }
                });

                tokio::spawn(batch_writes.then(|res| -> Result<(), ()> {
                    if let Err(()) = res {
                        panic!("blobstore sync queue writer unexpectedly ended}");
                    }
                    Ok(())
                }));
                Ok(())
            }
        })
        .boxify()
        .shared();

        Self {
            write_connection,
            read_connection,
            read_master_connection,
            write_sender: Arc::new(sender),
            ensure_worker_scheduled,
        }
    }

    fn get_up_query() -> &'static str {
        include_str!("../schemas/sqlite-blobstore-sync-queue.sql")
    }
}

const WRITE_BUFFER_SIZE: usize = 1000;

fn insert_entries(
    write_connection: Arc<Connection>,
    entries: Vec<BlobstoreSyncQueueEntry>,
) -> BoxFuture<(), Error> {
    let entries: Vec<_> = entries
        .into_iter()
        .map(|entry| {
            let BlobstoreSyncQueueEntry {
                repo_id,
                blobstore_key,
                blobstore_id,
                timestamp,
                ..
            } = entry;
            let t: Timestamp = timestamp.into();
            (repo_id, blobstore_key, blobstore_id, t)
        })
        .collect();

    let entries_ref: Vec<_> = entries
        .iter()
        .map(|(a, b, c, d)| (a, b, c, d)) // &(a, b, ...) into (&a, &b, ...)
        .collect();

    InsertEntry::query(&write_connection, entries_ref.as_ref())
        .map(|_| ())
        .boxify()
}

impl BlobstoreSyncQueue for SqlBlobstoreSyncQueue {
    fn add(&self, _ctx: CoreContext, entry: BlobstoreSyncQueueEntry) -> BoxFuture<(), Error> {
        STATS::adds.add_value(1);

        cloned!(self.write_sender);
        self.ensure_worker_scheduled
            .clone()
            .then(move |res| match res {
                Ok(_) => {
                    let (send, recv) = oneshot::channel();
                    try_boxfuture!(write_sender.unbounded_send((send, entry)));

                    recv.map_err(|err| err_msg(format!("failed to receive result {}", err)))
                        .and_then(|res| res)
                        .boxify()
                }
                Err(_) => {
                    panic!("failed to schedule write worker for BlobstoreSyncQueue");
                }
            })
            .boxify()
    }

    fn iter(
        &self,
        _ctx: CoreContext,
        repo_id: RepositoryId,
        older_than: DateTime,
        limit: usize,
    ) -> BoxFuture<Vec<BlobstoreSyncQueueEntry>, Error> {
        STATS::iters.add_value(1);
        // query
        GetRangeOfEntries::query(
            &self.read_master_connection,
            &repo_id,
            &older_than.into(),
            &limit,
        )
        .map(|rows| {
            rows.into_iter()
                .map(|(repo_id, blobstore_key, blobstore_id, timestamp, id)| {
                    BlobstoreSyncQueueEntry {
                        repo_id,
                        blobstore_key,
                        blobstore_id,
                        timestamp: timestamp.into(),
                        id: Some(id),
                    }
                })
                .collect()
        })
        .boxify()
    }

    fn del(
        &self,
        _ctx: CoreContext,
        entries: Vec<BlobstoreSyncQueueEntry>,
    ) -> BoxFuture<(), Error> {
        STATS::dels.add_value(1);

        let ids: Result<Vec<u64>, Error> = entries
            .into_iter()
            .map(|entry| {
                entry.id.ok_or_else(|| {
                    format_err!("BlobstoreSyncQueueEntry must contain `id` to be able to delete it")
                })
            })
            .collect();
        ids.into_future()
            .and_then({
                cloned!(self.write_connection);
                move |ids| {
                    future::join_all(ids.into_iter().map({
                        cloned!(write_connection);
                        move |id| DeleteEntry::query(&write_connection, &id)
                    }))
                }
            })
            .map(|_| ())
            .boxify()
    }

    fn get(
        &self,
        _ctx: CoreContext,
        repo_id: RepositoryId,
        key: String,
    ) -> BoxFuture<Vec<BlobstoreSyncQueueEntry>, Error> {
        GetByKey::query(&self.read_master_connection, &repo_id, &key)
            .map(|rows| {
                rows.into_iter()
                    .map(|(repo_id, blobstore_key, blobstore_id, timestamp, id)| {
                        BlobstoreSyncQueueEntry {
                            repo_id,
                            blobstore_key,
                            blobstore_id,
                            timestamp: timestamp.into(),
                            id: Some(id),
                        }
                    })
                    .collect()
            })
            .boxify()
    }
}
