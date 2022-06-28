/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::facebook::*;
use crate::replication::ReplicaLag;
use crate::replication::ReplicaLagMonitor;
use crate::*;

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use fbinit::FacebookInit;
use std::time::Duration;

macro_rules! fb_unimplemented {
    () => {
        unimplemented!("This is implemented only for fbcode_build!")
    };
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
    _mysql_options: MysqlOptions,
    _label: String,
    _tier: String,
    _readonly: bool,
) -> Result<SqlConnections, Error> {
    fb_unimplemented!()
}

pub fn create_mysql_connections_sharded<S>(
    _fb: FacebookInit,
    _global_connection_pool: MysqlOptions,
    _label: String,
    _shardmap: String,
    _shards: S,
    _readonly: bool,
) -> Result<SqlShardedConnections, Error>
where
    S: IntoIterator<Item = usize> + Clone,
{
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
