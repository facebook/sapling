/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::replication::{ReplicaLag, ReplicaLagMonitor};
use crate::{facebook::*, *};

use anyhow::{Error, Result};
use async_trait::async_trait;
use fbinit::FacebookInit;
use futures_ext::BoxFuture;
use slog::Logger;
use std::time::Duration;

macro_rules! fb_unimplemented {
    () => {
        unimplemented!("This is implemented only for fbcode_build!")
    };
}

impl PoolSizeConfig {
    pub fn for_regular_connection() -> Self {
        fb_unimplemented!()
    }

    pub fn for_sharded_connection() -> Self {
        fb_unimplemented!()
    }
}

pub fn create_myrouter_connections(
    _: String,
    _: Option<usize>,
    _: u16,
    _: ReadConnectionType,
    _: PoolSizeConfig,
    _: String,
    _: bool,
) -> SqlConnections {
    fb_unimplemented!()
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PoolConfig;

impl PoolConfig {
    pub fn new(
        _size: usize,
        _threads_num: i32,
        _per_key_limit: u64,
        _conn_age_timeout_ms: u64,
        _conn_idle_timeout_ms: u64,
        _conn_open_timeout_ms: u64,
        _query_time_limit: Duration,
    ) -> Self {
        Self
    }

    pub fn default() -> Self {
        Self
    }
}

#[derive(Clone, Default)]
pub struct SharedConnectionPool;
impl SharedConnectionPool {
    pub fn new() -> Self {
        Self
    }
}

pub fn create_mysql_connections_unsharded(
    _fb: FacebookInit,
    _connection_pool: SharedConnectionPool,
    _pool_config: PoolConfig,
    _tier: String,
    _read_con_type: ReadConnectionType,
    _readonly: bool,
) -> Result<SqlConnections, Error> {
    fb_unimplemented!()
}

pub fn deprecated_create_mysql_pool_unsharded(
    _fb: FacebookInit,
    _tier: String,
    _read_con_type: ReadConnectionType,
    _pool_size_config: PoolSizeConfig,
    _readonly: bool,
) -> Result<SqlConnections> {
    fb_unimplemented!()
}

pub fn create_mysql_connections_sharded<S>(
    _fb: FacebookInit,
    _connection_pool: SharedConnectionPool,
    _pool_config: PoolConfig,
    _shardmap: String,
    _shards: S,
    _read_con_type: ReadConnectionType,
    _readonly: bool,
) -> Result<SqlShardedConnections, Error>
where
    S: IntoIterator<Item = usize> + Clone,
{
    fb_unimplemented!()
}

pub async fn myrouter_ready(
    db_addr_opt: Option<String>,
    mysql_options: &MysqlOptions,
    _: &Logger,
) -> Result<(), Error> {
    if db_addr_opt.is_none() {
        return Ok(());
    };

    match mysql_options.connection_type {
        MysqlConnectionType::Myrouter(_) => {}
        _ => {
            return Ok(());
        }
    }

    fb_unimplemented!()
}

pub fn create_raw_xdb_connections(
    _: FacebookInit,
    _: String,
    _: ReadConnectionType,
    _: bool,
) -> BoxFuture<SqlConnections, Error> {
    fb_unimplemented!()
}

pub struct MyAdmin;
pub struct MyAdminLagMonitor;

impl MyAdmin {
    pub fn new(_: FacebookInit) -> Result<Self> {
        fb_unimplemented!()
    }

    pub fn single_shard_lag_monitor(&self, _: String) -> MyAdminLagMonitor {
        fb_unimplemented!()
    }

    pub fn shardmap_lag_monitor(&self, _: String) -> MyAdminLagMonitor {
        fb_unimplemented!()
    }
}

#[async_trait]
impl ReplicaLagMonitor for MyAdminLagMonitor {
    async fn get_replica_lag(&self) -> Result<Vec<ReplicaLag>> {
        fb_unimplemented!()
    }
}
