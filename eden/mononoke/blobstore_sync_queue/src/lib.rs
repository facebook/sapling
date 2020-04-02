/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{format_err, Error};
use cloned::cloned;
use context::CoreContext;
use futures::sync::{mpsc, oneshot};
use futures::{future, stream, Future, Stream};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt, StreamExt};
use metaconfig_types::{BlobstoreId, MultiplexId};
use mononoke_types::{DateTime, Timestamp};
use sql::mysql_async::{
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};
use sql::{queries, Connection};
pub use sql_construct::SqlConstruct;
pub use sql_ext::SqlConnections;
use stats::prelude::*;
use std::iter::IntoIterator;
use std::sync::Arc;
use uuid::Uuid;

define_stats! {
    prefix = "mononoke.blobstore_sync_queue";
    adds: timeseries(Rate, Sum),
    iters: timeseries(Rate, Sum),
    dels: timeseries(Rate, Sum),
}

// Identifier for given blobstore operation to faciliate correlating same operation
// across multiple blobstores.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct OperationKey(pub Uuid);
impl OperationKey {
    pub fn gen() -> OperationKey {
        OperationKey(Uuid::new_v4())
    }

    pub fn is_null(&self) -> bool {
        self == &OperationKey(Uuid::nil())
    }
}

impl From<OperationKey> for Value {
    fn from(id: OperationKey) -> Self {
        let OperationKey(uuid) = id;
        Value::Bytes(uuid.as_bytes().to_vec())
    }
}

impl ConvIr<OperationKey> for OperationKey {
    fn new(v: Value) -> Result<Self, FromValueError> {
        match v {
            Value::Bytes(bytes) => Ok(OperationKey(
                Uuid::from_slice(&bytes[..])
                    .map_err(move |_| FromValueError(Value::Bytes(bytes)))?,
            )),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> Self {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for OperationKey {
    type Intermediate = OperationKey;
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BlobstoreSyncQueueEntry {
    pub blobstore_key: String,
    pub blobstore_id: BlobstoreId,
    pub multiplex_id: MultiplexId,
    pub timestamp: DateTime,
    pub id: Option<u64>,
    pub operation_key: OperationKey,
}

impl BlobstoreSyncQueueEntry {
    pub fn new(
        blobstore_key: String,
        blobstore_id: BlobstoreId,
        multiplex_id: MultiplexId,
        timestamp: DateTime,
        operation_key: OperationKey,
    ) -> Self {
        Self {
            blobstore_key,
            blobstore_id,
            multiplex_id,
            timestamp,
            operation_key,
            id: None,
        }
    }
}

pub trait BlobstoreSyncQueue: Send + Sync {
    fn add(&self, ctx: CoreContext, entry: BlobstoreSyncQueueEntry) -> BoxFuture<(), Error> {
        self.add_many(ctx, Box::new(vec![entry].into_iter()))
    }

    fn add_many(
        &self,
        ctx: CoreContext,
        entries: Box<dyn Iterator<Item = BlobstoreSyncQueueEntry> + Send>,
    ) -> BoxFuture<(), Error>;

    /// Returns list of entries that consist of two groups of entries:
    /// 1. Group with at most `limit` entries that are older than `older_than` and
    ///    optionally sql like `key_like`
    /// 2. Group of entries whose `blobstore_key` can be found in group (1)
    ///
    /// As a result the caller gets a reasonably limited slice of BlobstoreSyncQueue entries that
    /// are all related, so that the caller doesn't need to fetch more data from BlobstoreSyncQueue
    /// to process the sync queue.
    fn iter(
        &self,
        ctx: CoreContext,
        key_like: Option<String>,
        multiplex_id: MultiplexId,
        older_than: DateTime,
        limit: usize,
    ) -> BoxFuture<Vec<BlobstoreSyncQueueEntry>, Error>;

    fn del(&self, ctx: CoreContext, entries: Vec<BlobstoreSyncQueueEntry>) -> BoxFuture<(), Error>;

    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Vec<BlobstoreSyncQueueEntry>, Error>;
}

impl BlobstoreSyncQueue for Arc<dyn BlobstoreSyncQueue> {
    fn add_many(
        &self,
        ctx: CoreContext,
        entries: Box<dyn Iterator<Item = BlobstoreSyncQueueEntry> + Send>,
    ) -> BoxFuture<(), Error> {
        (**self).add_many(ctx, entries)
    }

    fn iter(
        &self,
        ctx: CoreContext,
        key_like: Option<String>,
        multiplex_id: MultiplexId,
        older_than: DateTime,
        limit: usize,
    ) -> BoxFuture<Vec<BlobstoreSyncQueueEntry>, Error> {
        (**self).iter(ctx, key_like, multiplex_id, older_than, limit)
    }

    fn del(&self, ctx: CoreContext, entries: Vec<BlobstoreSyncQueueEntry>) -> BoxFuture<(), Error> {
        (**self).del(ctx, entries)
    }

    fn get(&self, ctx: CoreContext, key: String) -> BoxFuture<Vec<BlobstoreSyncQueueEntry>, Error> {
        (**self).get(ctx, key)
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
        blobstore_key: String,
        blobstore_id: BlobstoreId,
        multiplex_id: MultiplexId,
        timestamp: Timestamp,
        operation_key: OperationKey,
    )) {
        insert_or_ignore,
        "{insert_or_ignore}
         INTO blobstore_sync_queue (blobstore_key, blobstore_id, multiplex_id, add_timestamp, operation_key)
         VALUES {values}"
    }

    write DeleteEntries(>list ids: u64) {
        none,
        "DELETE FROM blobstore_sync_queue WHERE id in {ids}"
    }

    read GetRangeOfEntries(multiplex_id: MultiplexId, older_than: Timestamp, limit: usize) -> (
        String,
        BlobstoreId,
        MultiplexId,
        Timestamp,
        OperationKey,
        u64,
    ) {
        "SELECT blobstore_sync_queue.blobstore_key, blobstore_id, multiplex_id, add_timestamp, operation_key, id
         FROM blobstore_sync_queue
         JOIN (
               SELECT DISTINCT blobstore_key
               FROM blobstore_sync_queue
               WHERE add_timestamp <= {older_than} AND multiplex_id = {multiplex_id}
               LIMIT {limit}
         ) b
         ON blobstore_sync_queue.blobstore_key = b.blobstore_key AND multiplex_id = {multiplex_id}
         "
    }

    read GetRangeOfEntriesLike(blobstore_key_like: String, multiplex_id: MultiplexId, older_than: Timestamp, limit: usize) -> (
        String,
        BlobstoreId,
        MultiplexId,
        Timestamp,
        OperationKey,
        u64,
    ) {
        "SELECT blobstore_sync_queue.blobstore_key, blobstore_id, multiplex_id, add_timestamp, operation_key, id
         FROM blobstore_sync_queue
         JOIN (
               SELECT DISTINCT blobstore_key
               FROM blobstore_sync_queue
               WHERE blobstore_key LIKE {blobstore_key_like} AND add_timestamp <= {older_than} AND multiplex_id = {multiplex_id}
               LIMIT {limit}
         ) b
         ON blobstore_sync_queue.blobstore_key = b.blobstore_key AND multiplex_id = {multiplex_id}
         "
    }

    read GetByKey(key: String) -> (
        String,
        BlobstoreId,
        MultiplexId,
        Timestamp,
        OperationKey,
        u64,
    ) {
        "SELECT blobstore_key, blobstore_id, multiplex_id, add_timestamp, operation_key, id
         FROM blobstore_sync_queue
         WHERE blobstore_key = {key}"
    }
}

impl SqlConstruct for SqlBlobstoreSyncQueue {
    const LABEL: &'static str = "blobstore_sync_queue";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-blobstore-sync-queue.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        let write_connection = Arc::new(connections.write_connection);
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
                                        let _ = sender.send(Err(Error::msg(s.clone())));
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
            read_connection: connections.read_connection,
            read_master_connection: connections.read_master_connection,
            write_sender: Arc::new(sender),
            ensure_worker_scheduled,
        }
    }
}

const WRITE_BUFFER_SIZE: usize = 5000;

fn insert_entries(
    write_connection: Arc<Connection>,
    entries: Vec<BlobstoreSyncQueueEntry>,
) -> BoxFuture<(), Error> {
    let entries: Vec<_> = entries
        .into_iter()
        .map(|entry| {
            let BlobstoreSyncQueueEntry {
                blobstore_key,
                blobstore_id,
                timestamp,
                multiplex_id,
                operation_key,
                ..
            } = entry;
            let t: Timestamp = timestamp.into();
            (blobstore_key, blobstore_id, multiplex_id, t, operation_key)
        })
        .collect();

    let entries_ref: Vec<_> = entries
        .iter()
        .map(|(b, c, d, e, f)| (b, c, d, e, f)) // &(a, b, ...) into (&a, &b, ...)
        .collect();

    InsertEntry::query(&write_connection, entries_ref.as_ref())
        .map(|_| ())
        .boxify()
}

impl BlobstoreSyncQueue for SqlBlobstoreSyncQueue {
    fn add_many(
        &self,
        _ctx: CoreContext,
        entries: Box<dyn Iterator<Item = BlobstoreSyncQueueEntry> + Send>,
    ) -> BoxFuture<(), Error> {
        cloned!(self.write_sender);
        self.ensure_worker_scheduled
            .clone()
            .then(move |res| match res {
                Ok(_) => {
                    let (senders_entries, receivers): (Vec<_>, Vec<_>) = entries
                        .map(|entry| {
                            let (sender, receiver) = oneshot::channel();
                            ((sender, entry), receiver)
                        })
                        .unzip();

                    STATS::adds.add_value(senders_entries.len() as i64);
                    let r: Result<_, _> = senders_entries
                        .into_iter()
                        .map(|(send, entry)| write_sender.unbounded_send((send, entry)))
                        .collect();
                    let _ = try_boxfuture!(r);
                    future::join_all(receivers)
                        .map_err(|errs| format_err!("failed to receive result {:?}", errs))
                        .and_then(|results| {
                            let errs: Vec<_> =
                                results.into_iter().filter_map(|r| r.err()).collect();
                            if errs.len() > 0 {
                                future::err(format_err!("failed to receive result {:?}", errs))
                            } else {
                                future::ok(())
                            }
                        })
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
        key_like: Option<String>,
        multiplex_id: MultiplexId,
        older_than: DateTime,
        limit: usize,
    ) -> BoxFuture<Vec<BlobstoreSyncQueueEntry>, Error> {
        STATS::iters.add_value(1);
        let query = match &key_like {
            Some(sql_like) => GetRangeOfEntriesLike::query(
                &self.read_master_connection,
                &sql_like,
                &multiplex_id,
                &older_than.into(),
                &limit,
            )
            .left_future(),
            None => GetRangeOfEntries::query(
                &self.read_master_connection,
                &multiplex_id,
                &older_than.into(),
                &limit,
            )
            .right_future(),
        };

        query
            .map(|rows| {
                rows.into_iter()
                    .map(
                        |(
                            blobstore_key,
                            blobstore_id,
                            multiplex_id,
                            timestamp,
                            operation_key,
                            id,
                        )| {
                            BlobstoreSyncQueueEntry {
                                blobstore_key,
                                blobstore_id,
                                multiplex_id,
                                timestamp: timestamp.into(),
                                operation_key,
                                id: Some(id),
                            }
                        },
                    )
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

        let ids = try_boxfuture!(ids);

        stream::iter_ok(ids)
            .chunks(10_000)
            .and_then({
                cloned!(self.write_connection);
                move |chunk: Vec<u64>| DeleteEntries::query(&write_connection, &chunk[..])
            })
            .for_each(|_| Ok(()))
            .boxify()
    }

    fn get(
        &self,
        _ctx: CoreContext,
        key: String,
    ) -> BoxFuture<Vec<BlobstoreSyncQueueEntry>, Error> {
        GetByKey::query(&self.read_master_connection, &key)
            .map(|rows| {
                rows.into_iter()
                    .map(
                        |(
                            blobstore_key,
                            blobstore_id,
                            multiplex_id,
                            timestamp,
                            operation_key,
                            id,
                        )| {
                            BlobstoreSyncQueueEntry {
                                blobstore_key,
                                blobstore_id,
                                multiplex_id,
                                timestamp: timestamp.into(),
                                operation_key,
                                id: Some(id),
                            }
                        },
                    )
                    .collect()
            })
            .boxify()
    }
}
