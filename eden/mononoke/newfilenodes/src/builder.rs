/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use caching_ext::CacheHandlerFactory;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use metaconfig_types::ShardableRemoteDatabaseConfig;
use mononoke_types::RepositoryId;
use sql::Connection;
use sql_construct::SqlShardableConstructFromMetadataDatabaseConfig;
use sql_construct::SqlShardedConstruct;
use sql_ext::SqlShardedConnections;

use crate::local_cache::LocalCache;
use crate::reader::FilenodesReader;
use crate::remote_cache::RemoteCache;
use crate::writer::FilenodesWriter;
use crate::NewFilenodes;

pub const MYSQL_INSERT_CHUNK_SIZE: usize = 1000;
pub const SQLITE_INSERT_CHUNK_SIZE: usize = 100;

pub struct NewFilenodesBuilder {
    shard_connections: SqlShardedConnections,
    caches: Option<(LocalCache, RemoteCache)>,
}

impl SqlShardedConstruct for NewFilenodesBuilder {
    const LABEL: &'static str = "shardedfilenodes";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-filenodes.sql");

    fn from_sql_shard_connections(shard_connections: SqlShardedConnections) -> Self {
        Self {
            shard_connections,
            caches: None,
        }
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
    pub fn build(self, repo_id: RepositoryId) -> Result<NewFilenodes> {
        let SqlShardedConnections {
            read_connections,
            read_master_connections,
            write_connections,
        } = self.shard_connections;

        let chunk_size = match read_connections.get(0) {
            Some(Connection::Mysql(_)) => MYSQL_INSERT_CHUNK_SIZE,
            _ => SQLITE_INSERT_CHUNK_SIZE,
        };

        let mut reader = FilenodesReader::new(read_connections.clone(), read_master_connections)?;
        let writer = FilenodesWriter::new(chunk_size, write_connections, read_connections);

        if let Some((local_cache, remote_cache)) = self.caches {
            reader.local_cache = local_cache;
            reader.remote_cache = remote_cache;
        }

        Ok(NewFilenodes {
            reader: Arc::new(reader),
            writer: Arc::new(writer),
            repo_id,
        })
    }

    pub fn enable_caching(
        &mut self,
        cache_handler_factory: CacheHandlerFactory,
        history_cache_handler_factory: CacheHandlerFactory,
        backing_store_name: &str,
        backing_store_params: &str,
    ) -> Result<()> {
        // We require two cache builders for the two cache pools.
        let local_cache = LocalCache::new(&cache_handler_factory, &history_cache_handler_factory);

        // However, memcache doesn't have cache pools, so we can just use
        // either of the cache builders to construct the remote cache.
        let remote_cache = RemoteCache::new(
            &cache_handler_factory,
            backing_store_name,
            backing_store_params,
        )?;

        self.caches = Some((local_cache, remote_cache));

        Ok(())
    }
}
