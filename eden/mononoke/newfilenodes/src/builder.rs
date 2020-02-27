/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use cachelib::VolatileLruCachePool;
use fbinit::FacebookInit;
use futures::future::{join_all, Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt as _};
use sql::{rusqlite::Connection as SqliteConnection, Connection};
use sql_ext::{
    create_myrouter_connections, create_raw_xdb_connections, MysqlOptions, PoolSizeConfig,
    SqlConnections, SqlConstructors,
};
use sql_facebook::{myrouter, raw};
use std::sync::Arc;

use crate::local_cache::{CachelibCache, LocalCache};
use crate::reader::FilenodesReader;
use crate::writer::FilenodesWriter;
use crate::NewFilenodes;

pub const MYSQL_INSERT_CHUNK_SIZE: usize = 1000;
pub const SQLITE_INSERT_CHUNK_SIZE: usize = 100;

pub struct NewFilenodesBuilder {
    reader: FilenodesReader,
    writer: FilenodesWriter,
}

impl SqlConstructors for NewFilenodesBuilder {
    const LABEL: &'static str = "filenodes";

    fn from_connections(
        write_connection: Connection,
        read_connection: Connection,
        read_master_connection: Connection,
    ) -> Self {
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

    fn get_up_query() -> &'static str {
        include_str!("../schemas/sqlite-filenodes.sql")
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
        filenodes_cache_pool: VolatileLruCachePool,
        filenodes_history_cache_pool: VolatileLruCachePool,
    ) {
        self.reader.local_cache = LocalCache::Cachelib(CachelibCache::new(
            filenodes_cache_pool,
            filenodes_history_cache_pool,
        ));
    }

    pub fn with_sharded_xdb(
        fb: FacebookInit,
        tier: String,
        options: MysqlOptions,
        shard_count: usize,
        readonly: bool,
    ) -> BoxFuture<Self, Error> {
        match options.myrouter_port {
            Some(myrouter_port) => Self::with_sharded_myrouter(
                tier,
                myrouter_port,
                options.myrouter_read_service_type(),
                shard_count,
                readonly,
            ),
            None => Self::with_sharded_raw_xdb(
                fb,
                tier,
                options.db_locator_read_instance_requirement(),
                shard_count,
                readonly,
            ),
        }
    }

    pub fn with_sharded_sqlite(shard_count: usize) -> Result<Self, Error> {
        let mut read_connections = vec![];
        let mut read_master_connections = vec![];
        let mut write_connections = vec![];

        for _ in 0..shard_count {
            let con = SqliteConnection::open_in_memory()?;
            con.execute_batch(Self::get_up_query())?;
            let con = Connection::with_sqlite(con);

            read_connections.push(con.clone());
            read_master_connections.push(con.clone());
            write_connections.push(con);
        }

        let reader = FilenodesReader::new(read_connections.clone(), read_master_connections);

        let writer = FilenodesWriter::new(
            SQLITE_INSERT_CHUNK_SIZE,
            write_connections,
            read_connections,
        );

        Ok(Self { writer, reader })
    }

    fn with_sharded_myrouter(
        tier: String,
        port: u16,
        read_service_type: myrouter::ServiceType,
        shard_count: usize,
        readonly: bool,
    ) -> BoxFuture<Self, Error> {
        Self::with_sharded_factory(
            shard_count,
            move |shard_id| {
                Ok(create_myrouter_connections(
                    tier.clone(),
                    Some(shard_id),
                    port,
                    read_service_type,
                    // NOTE: We use for_regular_connection here, but we only use a small number of
                    // connections since they're semaphored under the hood.
                    PoolSizeConfig::for_regular_connection(),
                    "shardedfilenodes".into(),
                    readonly,
                ))
                .into_future()
                .boxify()
            },
            MYSQL_INSERT_CHUNK_SIZE,
        )
    }

    pub fn with_sharded_raw_xdb(
        fb: FacebookInit,
        tier: String,
        read_instance_requirement: raw::InstanceRequirement,
        shard_count: usize,
        readonly: bool,
    ) -> BoxFuture<Self, Error> {
        Self::with_sharded_factory(
            shard_count,
            move |shard_id| {
                create_raw_xdb_connections(
                    fb,
                    format!("{}.{}", tier, shard_id),
                    read_instance_requirement,
                    readonly,
                )
                .boxify()
            },
            MYSQL_INSERT_CHUNK_SIZE,
        )
    }

    fn with_sharded_factory(
        shard_count: usize,
        factory: impl Fn(usize) -> BoxFuture<SqlConnections, Error>,
        chunk_size: usize,
    ) -> BoxFuture<Self, Error> {
        let futs: Vec<_> = (1..=shard_count)
            .into_iter()
            .map(|shard| factory(shard))
            .collect();

        join_all(futs)
            .map(move |shard_connections| {
                let mut write_connections = vec![];
                let mut read_connections = vec![];
                let mut read_master_connections = vec![];

                for conn in shard_connections {
                    let SqlConnections {
                        write_connection,
                        read_connection,
                        read_master_connection,
                    } = conn;

                    write_connections.push(write_connection);
                    read_connections.push(read_connection);
                    read_master_connections.push(read_master_connection);
                }

                let reader =
                    FilenodesReader::new(read_connections.clone(), read_master_connections);

                let writer = FilenodesWriter::new(chunk_size, write_connections, read_connections);

                Self { writer, reader }
            })
            .boxify()
    }
}
