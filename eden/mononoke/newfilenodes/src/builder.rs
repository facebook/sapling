/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cachelib::VolatileLruCachePool;
use fbinit::FacebookInit;
use metaconfig_types::{RemoteMetadataDatabaseConfig, ShardableRemoteDatabaseConfig};
use sql::Connection;
use sql_construct::{
    SqlConstruct, SqlShardableConstructFromMetadataDatabaseConfig, SqlShardedConstruct,
};
use sql_ext::SqlConnections;
use std::sync::Arc;

use crate::local_cache::{CachelibCache, LocalCache};
use crate::reader::FilenodesReader;
use crate::remote_cache::{MemcacheCache, RemoteCache};
use crate::writer::FilenodesWriter;
use crate::NewFilenodes;

pub const MYSQL_INSERT_CHUNK_SIZE: usize = 1000;
pub const SQLITE_INSERT_CHUNK_SIZE: usize = 100;

pub struct NewFilenodesBuilder {
    reader: FilenodesReader,
    writer: FilenodesWriter,
}

impl SqlConstruct for NewFilenodesBuilder {
    const LABEL: &'static str = "filenodes";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-filenodes.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        let SqlConnections {
            write_connection,
            read_connection,
            read_master_connection,
        } = connections;
        let chunk_size = match read_connection {
            Connection::Sqlite(_) => SQLITE_INSERT_CHUNK_SIZE,
            Connection::Mysql(_) => MYSQL_INSERT_CHUNK_SIZE,
        };

        let reader =
            FilenodesReader::new(vec![read_connection.clone()], vec![read_master_connection]);

        let writer =
            FilenodesWriter::new(chunk_size, vec![write_connection], vec![read_connection]);

        Self { reader, writer }
    }
}

impl SqlShardedConstruct for NewFilenodesBuilder {
    const LABEL: &'static str = "shardedfilenodes";

    const CREATION_QUERY: &'static str = <NewFilenodesBuilder as SqlConstruct>::CREATION_QUERY;

    fn from_sql_shard_connections(shard_connections: Vec<SqlConnections>) -> Self {
        if shard_connections.is_empty() {
            // It should be impossible for shard_connections to be empty, as the configured
            // number of shards was required to be non-zero.
            panic!("sharded database constructed with no shards");
        }
        let chunk_size = match shard_connections.iter().next() {
            Some(SqlConnections {
                read_connection: Connection::Mysql(_),
                ..
            }) => MYSQL_INSERT_CHUNK_SIZE,
            _ => SQLITE_INSERT_CHUNK_SIZE,
        };
        let mut write_connections = Vec::with_capacity(shard_connections.len());
        let mut read_connections = Vec::with_capacity(shard_connections.len());
        let mut read_master_connections = Vec::with_capacity(shard_connections.len());
        for connections in shard_connections.into_iter() {
            write_connections.push(connections.write_connection);
            read_connections.push(connections.read_connection);
            read_master_connections.push(connections.read_master_connection);
        }
        let reader = FilenodesReader::new(read_connections.clone(), read_master_connections);

        let writer = FilenodesWriter::new(chunk_size, write_connections, read_connections);

        Self { reader, writer }
    }
}

impl SqlShardableConstructFromMetadataDatabaseConfig for NewFilenodesBuilder {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&ShardableRemoteDatabaseConfig> {
        Some(&remote.filenodes)
    }
}

impl NewFilenodesBuilder {
    pub fn build(self) -> NewFilenodes {
        NewFilenodes {
            reader: Arc::new(self.reader),
            writer: Arc::new(self.writer),
        }
    }

    pub fn enable_caching(
        &mut self,
        fb: FacebookInit,
        filenodes_cache_pool: VolatileLruCachePool,
        filenodes_history_cache_pool: VolatileLruCachePool,
        backing_store_name: &str,
        backing_store_params: &str,
    ) {
        self.reader.local_cache = LocalCache::Cachelib(CachelibCache::new(
            filenodes_cache_pool,
            filenodes_history_cache_pool,
        ));

        self.reader.remote_cache = RemoteCache::Memcache(MemcacheCache::new(
            fb,
            backing_store_name,
            backing_store_params,
        ));
    }
}
