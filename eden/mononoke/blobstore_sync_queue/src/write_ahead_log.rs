/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use auto_impl::auto_impl;
use context::CoreContext;
use metaconfig_types::MultiplexId;
use mononoke_types::Timestamp;
use sql::Connection;
use sql_construct::SqlConstruct;
use sql_ext::SqlConnections;

use crate::OperationKey;

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
}

#[derive(Clone)]
pub struct SqlBlobstoreWal {
    #[allow(dead_code)]
    read_connection: Connection,
    #[allow(dead_code)]
    read_master_connection: Connection,
    #[allow(dead_code)]
    write_connection: Connection,
}

// TODO(aida): The trait is not complete yet, it's just an initial setup.
#[async_trait]
#[auto_impl(Arc, Box)]
pub trait BlobstoreWal: Send + Sync {
    async fn log<'a>(&'a self, ctx: &'a CoreContext, entry: BlobstoreWalEntry)
    -> Result<(), Error>;
}

#[async_trait]
impl BlobstoreWal for SqlBlobstoreWal {
    async fn log<'a>(
        &'a self,
        _ctx: &'a CoreContext,
        _entry: BlobstoreWalEntry,
    ) -> Result<(), Error> {
        unimplemented!();
    }
}

impl SqlConstruct for SqlBlobstoreWal {
    const LABEL: &'static str = "blobstore_wal";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-blobstore-wal.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            read_connection: connections.read_connection,
            read_master_connection: connections.read_master_connection,
            write_connection: connections.write_connection,
        }
    }
}
