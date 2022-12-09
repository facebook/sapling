/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::ops::Index;
use std::slice::SliceIndex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

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
use rand::Rng;
use rendezvous::ConfigurableRendezVousController;
use rendezvous::RendezVous;
use rendezvous::RendezVousOptions;
use rendezvous::RendezVousStats;
use shared_error::anyhow::IntoSharedError;
use shared_error::anyhow::SharedError;
use sql::Connection;
use sql::WriteResult;
use sql_construct::SqlShardedConstruct;
use sql_ext::mononoke_queries;
use sql_ext::SqlShardedConnections;
use tunables::tunables;
use vec1::Vec1;

const SQL_WAL_WRITE_BUFFER_SIZE: usize = 1000;

/// Row id of the entry, and which SQL shard it belongs to.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ReadInfo {
    /// Present if this entry was obtained from reading from SQL
    pub id: Option<u64>,
    /// Present on reads and also updated on writes
    pub shard_id: Option<usize>,
}

impl ReadInfo {
    fn into_present_tuple(self) -> Option<(u64, usize)> {
        self.id.zip(self.shard_id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BlobstoreWalEntry {
    pub blobstore_key: String,
    pub multiplex_id: MultiplexId,
    pub timestamp: Timestamp,
    pub read_info: ReadInfo,
    pub blob_size: u64,
    pub retry_count: u32,
}

impl BlobstoreWalEntry {
    pub fn new(
        blobstore_key: String,
        multiplex_id: MultiplexId,
        timestamp: Timestamp,
        blob_size: u64,
    ) -> Self {
        Self {
            blobstore_key,
            multiplex_id,
            timestamp,
            blob_size,
            read_info: ReadInfo {
                id: None,
                shard_id: None,
            },
            retry_count: 0,
        }
    }

    pub fn increment_retry(&mut self) {
        self.retry_count += 1;
    }

    fn into_sql_tuple(self) -> (String, MultiplexId, Timestamp, u64, u32) {
        let Self {
            blobstore_key,
            multiplex_id,
            timestamp,
            blob_size,
            retry_count,
            ..
        } = self;
        (
            blobstore_key,
            multiplex_id,
            timestamp,
            blob_size,
            retry_count,
        )
    }

    fn from_row(shard_id: usize, row: (String, MultiplexId, Timestamp, u64, u64, u32)) -> Self {
        let (blobstore_key, multiplex_id, timestamp, id, blob_size, retry_count) = row;
        Self {
            blobstore_key,
            multiplex_id,
            timestamp,
            read_info: ReadInfo {
                id: Some(id),
                shard_id: Some(shard_id),
            },
            blob_size,
            retry_count,
        }
    }
}

type QueueResult = Result<BlobstoreWalEntry, SharedError>;
type EnqueueSender = mpsc::UnboundedSender<(oneshot::Sender<QueueResult>, BlobstoreWalEntry)>;

const DEL_CHUNK: usize = 10_000;

#[derive(Clone)]
pub struct SqlBlobstoreWal {
    read_master_connections: Vec1<Connection>,
    write_connections: Arc<Vec1<Connection>>,
    /// Sending entry over the channel allows it to be queued till
    /// the worker is free and able to write new entries to Mysql.
    enqueue_entry_sender: EnqueueSender,
    /// Worker allows to enqueue new entries while there is already
    /// a write query to Mysql in-fight.
    ensure_worker_scheduled: Shared<BoxFuture<'static, ()>>,
    /// Used to cycle through shards when reading
    conn_idx: Arc<AtomicUsize>,
    /// Used to batch deletions together and not overwhelm db with too many queries
    delete_rendezvous: RendezVous<BlobstoreWalEntry, (), ConfigurableRendezVousController>,
}

impl SqlBlobstoreWal {
    fn setup_worker(
        write_connections: Arc<Vec1<Connection>>,
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
            let mut conn_idx = rand::thread_rng().gen_range(0..write_connections.len());
            let enqueued_writes = receiver.ready_chunks(SQL_WAL_WRITE_BUFFER_SIZE).for_each(
                move |batch /* (Sender, BlobstoreWalEntry) */| {
                    conn_idx += 1;
                    let shard_id = conn_idx % write_connections.len();
                    let write_connection = write_connections[shard_id].clone();
                    async move {
                        let (senders, entries): (Vec<_>, Vec<_>) = batch.into_iter().unzip();

                        let result = insert_entries(&write_connection, &entries).await;
                        let result = result
                            .map_err(|err| err.context("Failed to insert to WAL").shared_error());
                        // We don't really need WriteResult data as we write in batches
                        let result = result.map(|_write_result| ());

                        // Update the clients
                        senders.into_iter().zip(entries).for_each(|(s, mut entry)| {
                            entry.read_info.shard_id = Some(shard_id);
                            // ignore the error, because receiver might have gone
                            let _ = s.send(result.clone().map(|()| entry));
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

    /// Only read from a certain range of shards. Notice that writes still go to all shards.
    /// Fails if range is empty. Panics if range is out of bounds.
    pub fn with_read_range(
        self,
        range: impl SliceIndex<[Connection], Output = [Connection]>,
    ) -> Result<Self> {
        let read = self.read_master_connections.index(range);
        Ok(Self {
            read_master_connections: read.to_vec().try_into()?,
            ..self
        })
    }

    // This doesn't do any automatic rendezvous/queueing
    async fn inner_delete_by_key(
        write_connections: &[Connection],
        entries: HashSet<BlobstoreWalEntry>,
    ) -> Result<()> {
        // We're grouping by shard id AND multiplex id
        type GroupBy = (usize, MultiplexId);
        let mut del_info: Vec<(GroupBy, &String)> = entries
            .iter()
            .map(|entry| {
                let shard_id = entry
                    .read_info
                    .shard_id
                    .context("BlobstoreWalEntry must have `shard_id` to delete by key")?;
                Ok(((shard_id, entry.multiplex_id), &entry.blobstore_key))
            })
            .collect::<Result<_>>()?;
        del_info.sort_unstable_by_key(|(group, _)| *group);
        stream::iter(
            del_info
                .group_by(|(group1, _), (group2, _)| group1 == group2)
                .map(|batch| async move {
                    let (shard_id, multiplex_id) = batch[0].0;
                    let del_entries: Vec<String> =
                        batch.iter().map(|(_, key)| (*key).clone()).collect();
                    for chunk in del_entries.chunks(DEL_CHUNK) {
                        WalDeleteKeys::query(&write_connections[shard_id], &multiplex_id, chunk)
                            .await?;
                    }
                    anyhow::Ok(())
                })
                .collect::<Vec<_>>(), // prevents compiler bug
        )
        .buffered(10)
        .try_collect::<()>()
        .await
    }
}

#[async_trait]
#[auto_impl(Arc, Box)]
pub trait BlobstoreWal: Send + Sync {
    async fn log<'a>(
        &'a self,
        ctx: &'a CoreContext,
        entry: BlobstoreWalEntry,
    ) -> Result<BlobstoreWalEntry> {
        self.log_many(ctx, vec![entry])
            .await?
            .into_iter()
            .next()
            .context("Missing entry")
    }

    async fn log_many<'a>(
        &'a self,
        ctx: &'a CoreContext,
        entry: Vec<BlobstoreWalEntry>,
    ) -> Result<Vec<BlobstoreWalEntry>>;

    async fn read<'a>(
        &'a self,
        ctx: &'a CoreContext,
        multiplex_id: &MultiplexId,
        older_than: &Timestamp,
        limit: usize,
    ) -> Result<Vec<BlobstoreWalEntry>>;

    /// Entries must have `id` and `shard_id` set (automatic when they are obtained from `read`)
    async fn delete<'a>(
        &'a self,
        ctx: &'a CoreContext,
        entries: &'a [BlobstoreWalEntry],
    ) -> Result<()>;

    /// Entries must have `shard_id` set (automatic when obtained from `log` or `log_many`)
    /// Will delete ALL entries with the same multiplex and key, independent of other fields.
    async fn delete_by_key(&self, ctx: &CoreContext, entries: &[BlobstoreWalEntry]) -> Result<()>;
}

#[async_trait]
impl BlobstoreWal for SqlBlobstoreWal {
    async fn log_many<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        entries: Vec<BlobstoreWalEntry>,
    ) -> Result<Vec<BlobstoreWalEntry>> {
        self.ensure_worker_scheduled.clone().await;

        // If we want to optimize, we can avoid creating a oneshot for each entry by batching together.
        let write_futs = entries
            .iter()
            .cloned()
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

        write_results
            .into_iter()
            .collect::<Result<_, _>>()
            .context("Failed to write to the SqlBlobstoreWal")
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
            let cur_shard = self.conn_idx.fetch_add(1, Ordering::Relaxed) % shards;
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
        let mut entry_info: Vec<(u64, usize)> =
            entries
                .iter()
                .map(|entry| {
                    entry.read_info.clone().into_present_tuple().context(
                        "BlobstoreWalEntry must contain `read_info` to be able to delete it",
                    )
                })
                .collect::<Result<_, _>>()?;
        entry_info.sort_unstable_by_key(|(_, shard_id)| *shard_id);
        stream::iter(
            entry_info
                .group_by(|(_, shard_id1), (_, shard_id2)| shard_id1 == shard_id2)
                .map(|batch| async move {
                    let shard_id: usize = batch[0].1;
                    let ids: Vec<u64> = batch.iter().map(|(id, _)| *id).collect();
                    for chunk in ids.chunks(10_000) {
                        WalDeleteEntries::query(&self.write_connections[shard_id], chunk).await?;
                    }
                    anyhow::Ok(())
                })
                .collect::<Vec<_>>(), // prevents compiler bug
        )
        .buffered(10)
        .try_collect::<()>()
        .await
    }

    async fn delete_by_key(&self, ctx: &CoreContext, entries: &[BlobstoreWalEntry]) -> Result<()> {
        if !tunables().get_wal_disable_rendezvous_on_deletes() {
            self.delete_rendezvous
                .dispatch(ctx.fb, entries.iter().cloned().collect(), || {
                    let connections = self.write_connections.clone();
                    |keys| async move {
                        Self::inner_delete_by_key(&connections, keys).await?;
                        // We don't care about results
                        Ok(HashMap::new())
                    }
                })
                .await?;
        } else {
            Self::inner_delete_by_key(&self.write_connections, entries.iter().cloned().collect())
                .await?;
        }
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
        let write_connections = Arc::new(write_connections);

        let (sender, ensure_worker_scheduled) =
            SqlBlobstoreWal::setup_worker(write_connections.clone());
        let conn_idx = rand::thread_rng().gen_range(0..read_master_connections.len());

        Self {
            write_connections,
            read_master_connections,
            enqueue_entry_sender: sender,
            ensure_worker_scheduled,
            conn_idx: Arc::new(AtomicUsize::new(conn_idx)),
            // For the delete rendezvous, we don't need to be super fast.
            // - 1 free connection just so we don't wait unnecessarily if traffic is very low
            // - It's fine to wait up to 5 secs to remove, though this likely won't happen.
            // - We're batching underlying requests at 10k
            delete_rendezvous: RendezVous::new(
                ConfigurableRendezVousController::new(
                    RendezVousOptions {
                        free_connections: 1,
                    },
                    || Duration::from_secs(5),
                    || DEL_CHUNK,
                ),
                Arc::new(RendezVousStats::new("wal_delete".to_owned())),
            ),
        }
    }
}

async fn insert_entries(
    write_connection: &Connection,
    entries: &[BlobstoreWalEntry],
) -> Result<WriteResult> {
    let entries: Vec<_> = entries
        .iter()
        .cloned()
        .map(|entry| entry.into_sql_tuple())
        .collect();
    let entries_ref: Vec<_> = entries
        .iter()
        .map(|(a, b, c, d, e)| (a, b, c, d, e)) // &(a, b, ...) into (&a, &b, ...)
        .collect();

    WalInsertEntry::query(write_connection, &entries_ref).await
}

mononoke_queries! {
    write WalDeleteEntries(>list ids: u64) {
        none,
        "DELETE FROM blobstore_write_ahead_log WHERE id in {ids}"
    }

    // This will delete ALL entries for given key. Used for the optimisation where we
    // remove keys from the queue if the write fully succeeded.
    // Ideally we wanted to use `(multiplex_id, blobstore_key) in {entries}` but that's not
    // possible using our SQL libraries, because everything must be a column value, and there are
    // no tuple/array column values (though you could write that query in raw SQL)
    write WalDeleteKeys(multiplex_id: MultiplexId, >list entries: String) {
        none,
        "DELETE FROM blobstore_write_ahead_log WHERE multiplex_id = {multiplex_id} AND blobstore_key in {entries}"
    }

    write WalInsertEntry(values: (
        blobstore_key: String,
        multiplex_id: MultiplexId,
        timestamp: Timestamp,
        blob_size: u64,
        retry_count: u32,
    )) {
        none,
        "INSERT INTO blobstore_write_ahead_log (blobstore_key, multiplex_id, timestamp, blob_size, retry_count)
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
        u64,
        u64,
        u32,
    ) {
        "SELECT blobstore_key, multiplex_id, timestamp, id, blob_size, retry_count
         FROM blobstore_write_ahead_log
         WHERE multiplex_id = {multiplex_id} AND timestamp <= {older_than}
         LIMIT {limit}"
    }
}
