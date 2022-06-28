/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Result;
use async_trait::async_trait;
use auto_impl::auto_impl;
use cloned::cloned;
use context::CoreContext;
use futures::channel::mpsc;
use futures::channel::oneshot;
use futures::future;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use futures::future::Shared;
use futures::future::TryFutureExt;
use futures::stream::StreamExt;
use metaconfig_types::MultiplexId;
use mononoke_types::Timestamp;
use shared_error::anyhow::IntoSharedError;
use shared_error::anyhow::SharedError;
use sql::Connection;
use sql::WriteResult;
use sql_construct::SqlConstruct;
use sql_ext::SqlConnections;
use std::sync::Arc;

use crate::queries;
use crate::OperationKey;

const SQL_WAL_WRITE_BUFFER_SIZE: usize = 1000;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct BlobstoreWalEntry {
    pub blobstore_key: String,
    pub multiplex_id: MultiplexId,
    pub timestamp: Timestamp,
    pub id: Option<u64>,
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
            id: None,
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
            id: Some(id),
            blob_size,
        }
    }
}

type EnqueueSender =
    mpsc::UnboundedSender<(oneshot::Sender<Result<(), SharedError>>, BlobstoreWalEntry)>;

#[derive(Clone)]
pub struct SqlBlobstoreWal {
    #[allow(dead_code)]
    read_connection: Connection,
    read_master_connection: Connection,
    #[allow(dead_code)]
    write_connection: Connection,
    /// Sending entry over the channel allows it to be queued till
    /// the worker is free and able to write new entries to Mysql.
    enqueue_entry_sender: Arc<EnqueueSender>,
    /// Worker allows to enqueue new entries while there is already
    /// a write query to Mysql in-fight.
    #[allow(dead_code)]
    ensure_worker_scheduled: Shared<BoxFuture<'static, ()>>,
}

impl SqlBlobstoreWal {
    fn setup_worker(
        write_connection: Connection,
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
            mpsc::unbounded::<(oneshot::Sender<Result<(), SharedError>>, BlobstoreWalEntry)>();

        let worker = async move {
            let enqueued_writes = receiver.ready_chunks(SQL_WAL_WRITE_BUFFER_SIZE).for_each(
                move |batch /* (Sender, BlobstoreWalEntry) */| {
                    cloned!(write_connection);
                    async move {
                        let (senders, entries): (Vec<_>, Vec<_>) = batch.into_iter().unzip();

                        let result = insert_entries(&write_connection, entries).await;
                        let result = result
                            .map_err(|err| err.context("Failed to insert to WAL").shared_error());
                        // We dont't really need WriteResult data as we write in batches
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
}

#[async_trait]
impl BlobstoreWal for SqlBlobstoreWal {
    async fn log_many<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        entries: Vec<BlobstoreWalEntry>,
    ) -> Result<()> {
        self.ensure_worker_scheduled.clone().await;

        let mut write_futs = Vec::with_capacity(entries.len());
        entries.into_iter().try_for_each(|entry| {
            let (sender, receiver) = oneshot::channel();
            write_futs.push(receiver);

            // Enqueue new entry
            self.enqueue_entry_sender.unbounded_send((sender, entry))
        })?;

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

        let errs: Vec<_> = write_results.into_iter().filter_map(|r| r.err()).collect();
        if !errs.is_empty() {
            // Actual errors that occurred while tryint to insert new entries to
            // the Mysql table.
            return Err(format_err!(
                "Failed to write to the SqlBlobstoreWal: {:?}",
                errs
            ));
        }

        Ok(())
    }

    async fn read<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        multiplex_id: &MultiplexId,
        older_than: &Timestamp,
        limit: usize,
    ) -> Result<Vec<BlobstoreWalEntry>> {
        let rows = queries::WalReadEntries::query(
            &self.read_master_connection,
            multiplex_id,
            older_than,
            &limit,
        )
        .await?;

        let entries = rows.into_iter().map(BlobstoreWalEntry::from_row).collect();
        Ok(entries)
    }
}

impl SqlConstruct for SqlBlobstoreWal {
    const LABEL: &'static str = "blobstore_wal";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-blobstore-wal.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        let SqlConnections {
            read_connection,
            read_master_connection,
            write_connection,
        } = connections;

        let (sender, ensure_worker_scheduled) =
            SqlBlobstoreWal::setup_worker(write_connection.clone());

        Self {
            read_connection,
            read_master_connection,
            write_connection,
            enqueue_entry_sender: Arc::new(sender),
            ensure_worker_scheduled,
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

    queries::WalInsertEntry::query(write_connection, &entries_ref).await
}
