/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::format_err;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use auto_impl::auto_impl;
use context::CoreContext;
use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::future;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::Shared;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use metaconfig_types::MultiplexId;
use mononoke_types::Timestamp;
use shared_error::anyhow::IntoSharedError;
use shared_error::anyhow::SharedError;
use sql::queries;
use sql::Connection;
use sql::WriteResult;
use sql_construct::SqlShardedConstruct;
use sql_ext::SqlShardedConnections;
use vec1::Vec1;

use crate::OperationKey;

const SQL_WAL_WRITE_BUFFER_SIZE: usize = 1000;

/// Row id of the entry, and which SQL shard it belongs to.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ReadInfo {
    id: u64,
    shard_id: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BlobstoreWalEntry {
    pub blobstore_key: String,
    pub multiplex_id: MultiplexId,
    pub timestamp: Timestamp,
    /// Present if this entry was obtained from reading from SQL
    pub read_info: Option<ReadInfo>,
    pub operation_key: OperationKey,
    pub blob_size: Option<u64>,
}

impl BlobstoreWalEntry {
    pub fn new(
        blobstore_key: String,
        multiplex_id: MultiplexId,
        timestamp: Timestamp,
        operation_key: OperationKey,
        blob_size: Option<u64>,
    ) -> Self {
        Self {
            blobstore_key,
            multiplex_id,
            timestamp,
            operation_key,
            blob_size,
            read_info: None,
        }
    }

    fn into_sql_tuple(self) -> (String, MultiplexId, Timestamp, OperationKey, Option<u64>) {
        let Self {
            blobstore_key,
            multiplex_id,
            timestamp,
            operation_key,
            blob_size,
            ..
        } = self;
        (
            blobstore_key,
            multiplex_id,
            timestamp,
            operation_key,
            blob_size,
        )
    }

    fn from_row(
        shard_id: usize,
        row: (
            String,
            MultiplexId,
            Timestamp,
            OperationKey,
            u64,
            Option<u64>,
        ),
    ) -> Self {
        let (blobstore_key, multiplex_id, timestamp, operation_key, id, blob_size) = row;
        Self {
            blobstore_key,
            multiplex_id,
            timestamp,
            operation_key,
            read_info: Some(ReadInfo { id, shard_id }),
            blob_size,
        }
    }
}

type QueueResult = Result<(), SharedError>;
type EnqueueSender = mpsc::UnboundedSender<(oneshot::Sender<QueueResult>, BlobstoreWalEntry)>;

#[derive(Clone)]
pub struct SqlBlobstoreWal {
    read_master_connections: Vec1<Connection>,
    write_connections: Vec1<Connection>,
    /// Sending entry over the channel allows it to be queued till
    /// the worker is free and able to write new entries to Mysql.
    enqueue_entry_sender: EnqueueSender,
    /// Worker allows to enqueue new entries while there is already
    /// a write query to Mysql in-fight.
    ensure_worker_scheduled: Shared<BoxFuture<'static, ()>>,
    /// Used to cycle through shards when reading
    shards_read: Arc<AtomicUsize>,
}

impl SqlBlobstoreWal {
    fn setup_worker(
        write_connections: Vec1<Connection>,
    ) -> (EnqueueSender, Shared<BoxFuture<'static, ()>>) {
        // The mpsc channel needed as a way to enqueue new entries while there is an
        // in-flight write query to Mysql.
        // - queue_sender will be used to add new entries to the queue (channel),
        // - receiver - to read a new batch of entries and write them to Mysql.
        //
        // To notify the clients back that the entry was successfully written to Mysql,
        // a oneshot channel is used. When the enqueued entries are written, the clients
        // receive result of the operation:
        // error if something went wrong and nothing if it's ok.
        let (queue_sender, receiver) =
            mpsc::unbounded::<(oneshot::Sender<QueueResult>, BlobstoreWalEntry)>();

        let worker = async move {
            let mut batch_count = 0usize;
            let enqueued_writes = receiver.ready_chunks(SQL_WAL_WRITE_BUFFER_SIZE).for_each(
                move |batch /* (Sender, BlobstoreWalEntry) */| {
                    batch_count += 1;
                    let write_connection =
                        write_connections[batch_count % write_connections.len()].clone();
                    async move {
                        let (senders, entries): (Vec<_>, Vec<_>) = batch.into_iter().unzip();

                        let result = insert_entries(&write_connection, entries).await;
                        let result = result
                            .map_err(|err| err.context("Failed to insert to WAL").shared_error());
                        // We don't really need WriteResult data as we write in batches
                        let result = result.map(|_write_result| ());

                        // Update the clients
                        senders.into_iter().for_each(|s| {
                            match s.send(result.clone()) {
                                Ok(_) => (),
                                Err(_) => { /* ignore the error, because receiver might have gone */
                                }
                            };
                        });
                    }
                },
            );
            tokio::spawn(enqueued_writes);
        }
        .boxed()
        .shared();

        (queue_sender, worker)
    }
}

#[async_trait]
#[auto_impl(Arc, Box)]
pub trait BlobstoreWal: Send + Sync {
    async fn log<'a>(&'a self, ctx: &'a CoreContext, entry: BlobstoreWalEntry) -> Result<()> {
        let _result = self.log_many(ctx, vec![entry]).await?;
        Ok(())
    }

    async fn log_many<'a>(
        &'a self,
        ctx: &'a CoreContext,
        entry: Vec<BlobstoreWalEntry>,
    ) -> Result<()>;

    async fn read<'a>(
        &'a self,
        ctx: &'a CoreContext,
        multiplex_id: &MultiplexId,
        older_than: &Timestamp,
        limit: usize,
    ) -> Result<Vec<BlobstoreWalEntry>>;

    async fn delete<'a>(
        &'a self,
        ctx: &'a CoreContext,
        entries: &'a [BlobstoreWalEntry],
    ) -> Result<()>;
}

#[async_trait]
impl BlobstoreWal for SqlBlobstoreWal {
    async fn log_many<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        entries: Vec<BlobstoreWalEntry>,
    ) -> Result<()> {
        self.ensure_worker_scheduled.clone().await;

        // If we want to optimize, we can avoid creating a oneshot for each entry by batching together.
        let write_futs = entries
            .into_iter()
            .map(|entry| {
                let (sender, receiver) = oneshot::channel();
                self.enqueue_entry_sender.unbounded_send((sender, entry))?;
                Ok(receiver)
            })
            .collect::<Result<Vec<oneshot::Receiver<QueueResult>>>>()?;

        let write_results = future::try_join_all(write_futs)
            .map_err(|err| {
                // If one of the futures fails to receive result from the sql wal,
                // we cannot be sure that the entries were written Mysql WAL table.
                format_err!(
                    "Failed to receive results from the SqlBlobstoreWal: {:?}",
                    err
                )
            })
            .await?;

        let errs: Vec<_> = write_results
            .into_iter()
            .filter_map(|r| r.err())
            // Let's not print too many errors
            .take(3)
            .collect();
        if !errs.is_empty() {
            // Actual errors that occurred while trying to insert new entries to
            // the Mysql table.
            return Err(format_err!(
                "Failed to write to the SqlBlobstoreWal: {:?}",
                errs,
            ));
        }

        Ok(())
    }

    async fn read<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        multiplex_id: &MultiplexId,
        older_than: &Timestamp,
        mut limit: usize,
    ) -> Result<Vec<BlobstoreWalEntry>> {
        let mut entries = Vec::new();
        // Traverse shards in order fetching from them.
        // To optimise this you can start multiple jobs, one for each shard.
        let shards = self.read_master_connections.len();
        for _ in 0..shards {
            let cur_shard = self.shards_read.fetch_add(1, Ordering::Relaxed) % shards;
            let rows = WalReadEntries::query(
                &self.read_master_connections[cur_shard],
                multiplex_id,
                older_than,
                &limit,
            )
            .await?;
            limit = limit.saturating_sub(rows.len());
            entries.extend(
                rows.into_iter()
                    .map(|r| BlobstoreWalEntry::from_row(cur_shard, r)),
            );
            if limit == 0 {
                break;
            }
        }
        Ok(entries)
    }

    async fn delete<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        entries: &'a [BlobstoreWalEntry],
    ) -> Result<()> {
        let mut entry_info: Vec<ReadInfo> = entries
            .iter()
            .map(|entry| {
                entry
                    .read_info
                    .clone()
                    .context("BlobstoreWalEntry must contain `read_info` to be able to delete it")
            })
            .collect::<Result<_, _>>()?;
        entry_info.sort_unstable_by_key(|info| info.shard_id);
        stream::iter(
            entry_info
                .group_by(|info1, info2| info1.shard_id == info2.shard_id)
                .map(|batch| async move {
                    let shard_id: usize = batch[0].shard_id;
                    let ids: Vec<u64> = batch.iter().map(|info| info.id).collect();
                    for chunk in ids.chunks(10_000) {
                        WalDeleteEntries::query(&self.write_connections[shard_id], chunk).await?;
                    }
                    anyhow::Ok(())
                })
                .collect::<Vec<_>>(), // prevents compiler bug
        )
        .buffered(10)
        .try_collect::<()>()
        .await?;

        Ok(())
    }
}

impl SqlShardedConstruct for SqlBlobstoreWal {
    const LABEL: &'static str = "blobstore_wal";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-blobstore-wal.sql");

    fn from_sql_shard_connections(connections: SqlShardedConnections) -> Self {
        let SqlShardedConnections {
            read_connections: _,
            read_master_connections,
            write_connections,
        } = connections;

        let (sender, ensure_worker_scheduled) =
            SqlBlobstoreWal::setup_worker(write_connections.clone());

        Self {
            write_connections,
            read_master_connections,
            enqueue_entry_sender: sender,
            ensure_worker_scheduled,
            shards_read: Arc::new(AtomicUsize::new(0)),
        }
    }
}

async fn insert_entries(
    write_connection: &Connection,
    entries: Vec<BlobstoreWalEntry>,
) -> Result<WriteResult> {
    let entries: Vec<_> = entries
        .into_iter()
        .map(|entry| entry.into_sql_tuple())
        .collect();
    let entries_ref: Vec<_> = entries
        .iter()
        .map(|(a, b, c, d, e)| (a, b, c, d, e)) // &(a, b, ...) into (&a, &b, ...)
        .collect();

    WalInsertEntry::query(write_connection, &entries_ref).await
}

queries! {
    write WalDeleteEntries(>list ids: u64) {
        none,
        "DELETE FROM blobstore_write_ahead_log WHERE id in {ids}"
    }

    write WalInsertEntry(values: (
        blobstore_key: String,
        multiplex_id: MultiplexId,
        timestamp: Timestamp,
        operation_key: OperationKey,
        blob_size: Option<u64>,
    )) {
        none,
        "INSERT INTO blobstore_write_ahead_log (blobstore_key, multiplex_id, timestamp, operation_key, blob_size)
         VALUES {values}"
    }

    // In comparison to the sync-queue, we write blobstore keys to the WAL only once
    // during the `put` operation. This way when the healer reads entries from the WAL,
    // it doesn't need to filter out distinct operation keys and then blobstore keys
    // (because each blobstore key can have multiple appearances with the same and
    // with different operation keys).
    // The healer can just read all the entries older than the timestamp and they will
    // represent a set of different put opertions by design.
    read WalReadEntries(multiplex_id: MultiplexId, older_than: Timestamp, limit: usize) -> (
        String,
        MultiplexId,
        Timestamp,
        OperationKey,
        u64,
        Option<u64>,
    ) {
        "SELECT blobstore_key, multiplex_id, timestamp, operation_key, id, blob_size
         FROM blobstore_write_ahead_log
         WHERE multiplex_id = {multiplex_id} AND timestamp <= {older_than}
         LIMIT {limit}
         "
    }
}
