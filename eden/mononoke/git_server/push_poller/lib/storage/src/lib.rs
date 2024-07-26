/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::RwLock;

use anyhow::Context;
use anyhow::Result;
use ephemeral_shard::EphemeralSchema;
use ephemeral_shard::EphemeralShard;
use fbinit::FacebookInit;
use maplit::hashset;
use mysql_client::BoundConnectionFactory;
use mysql_client::BoundConnectionPool;
use mysql_client::Connection as MysqlClientConnection;
use mysql_client::ConnectionOptions;
use mysql_client::ConnectionPool;
use mysql_client::ConnectionPoolOptions;
use mysql_client::DbLocator;
use mysql_client::DbLocatorStats;
use mysql_client::InstanceRequirement;
use mysql_client::MysqlCppClient;
use mysql_client::RegionRequirement;
use smc_models::shard::get_replicaset_for_shard;
use smc_models::SmcModelsClient;
use sql_common::mysql::Connection as MysqlConnection;
use sql_common::mysql::ConnectionStats;
use sql_common::Connection;
use sql_ext::SqlConnections;
use tracing::info;

const READ_ONLY_DB_ROLE: &str = "scriptro";
const READ_WRITE_DB_ROLE: &str = "scriptrw";

#[derive(Clone)]
pub enum Destination {
    Ephemeral(EphemeralSchema),
    Prod,
}

pub struct XdbFactory {
    fb: FacebookInit,
    destination: Destination,
    pool_options: ConnectionPoolOptions,
    conn_options: ConnectionOptions,
    connections: Arc<RwLock<HashMap<String, (Arc<Xdb>, Option<EphemeralShard>)>>>,
    smc_models_client: SmcModelsClient,
}

impl XdbFactory {
    pub fn new(
        fb: FacebookInit,
        destination: Destination,
        pool_options: ConnectionPoolOptions,
        conn_options: ConnectionOptions,
    ) -> Result<Self> {
        Ok(XdbFactory {
            fb,
            destination,
            pool_options,
            conn_options,
            connections: Arc::new(RwLock::new(HashMap::new())),
            smc_models_client: SmcModelsClient::new()?,
        })
    }

    async fn get_known_aliases_for_shard(&self, shard_name: &str) -> Result<HashSet<String>> {
        let replicaset_name = get_replicaset_for_shard(shard_name).await?;
        let replicaset = self.smc_models_client.replicaset(&replicaset_name).await?;
        let aliases = if let Some(db_name) = replicaset.database_for_shard(shard_name) {
            replicaset
                .shards_for_database(db_name)
                .into_iter()
                .map(|shard| shard.shard_id.to_string())
                .collect()
        } else {
            HashSet::new()
        };
        Ok(aliases)
    }

    pub async fn create_or_get_shard(&self, shard_name: &str) -> Result<Arc<Xdb>> {
        let (shard, guard) = match &self.destination {
            Destination::Ephemeral(ephemeral_schema) => {
                if self.connections.read().unwrap().get(shard_name).is_none() {
                    let shard =
                        EphemeralShard::new(self.fb, Some(shard_name), ephemeral_schema.clone())
                            .await
                            .context("Failed to get ephemeral shard")?;
                    info!("Using ephemeral shard `{shard}` for DB `{shard_name}`");
                    (shard.to_string(), Some(shard))
                } else {
                    (shard_name.to_string(), None)
                }
            }
            Destination::Prod => (shard_name.to_string(), None),
        };

        {
            let connections_guard = self.connections.read().unwrap();
            if let Some((db, _guard)) = connections_guard.get(shard_name) {
                return Ok(db.clone());
            }
        }

        let db = Arc::new(Xdb::new(
            self.fb,
            self.destination.clone(),
            &shard,
            &self.pool_options,
            self.conn_options.to_owned(),
        )?);

        let known_aliases = if matches!(self.destination, Destination::Ephemeral(_)) {
            self.get_known_aliases_for_shard(shard_name).await?
        } else {
            // The aliases are resolved automatically in production and the caching below can lead
            // to unexpected behaviour such as caching an alias that is no longer valid.
            hashset! {shard_name.to_string()}
        };

        let mut connections_guard = self.connections.write().unwrap();
        if let Some((db, _guard)) = connections_guard.get(shard_name) {
            return Ok(db.clone());
        }
        for known_alias in known_aliases {
            connections_guard.insert(known_alias, (db.clone(), guard.clone()));
        }

        Ok(db)
    }
}

pub struct Xdb {
    read_pool: BoundConnectionPool,
    read_stats: Arc<ConnectionStats>,
    write_pool: BoundConnectionPool,
    write_stats: Arc<ConnectionStats>,
}

impl Xdb {
    /// Constructs a new instance of `Xdb` by creating a conection pool for reads and writes.
    fn new(
        fb: FacebookInit,
        destination: Destination,
        xdb: &str,
        pool_options: &ConnectionPoolOptions,
        conn_options: ConnectionOptions,
    ) -> Result<Self> {
        let client = MysqlCppClient::new(fb).context("Failed to create MySQL c++ client")?;
        let pool = ConnectionPool::new_with_conn_options(&client, pool_options, conn_options)?;
        let mut write_locator =
            DbLocator::new_with_rolename(xdb, InstanceRequirement::Master, READ_WRITE_DB_ROLE)
                .context("Failed to create DbLocator")?;
        write_locator.set_stats(DbLocatorStats::new());

        let write_pool = pool.clone().bind(write_locator);

        let read_instance_requirement = if matches!(destination, Destination::Ephemeral(_)) {
            InstanceRequirement::Master
        } else {
            InstanceRequirement::ReplicaOnly
        };

        let mut read_locator =
            DbLocator::new_with_rolename(xdb, read_instance_requirement, READ_ONLY_DB_ROLE)
                .context("Failed to create DbLocator")?;
        // Attempt to connect to the closest region and fail-over to the next
        // closest region. Ignored for, and therefore not applied to, write
        // connections to the primary.
        read_locator.set_region_requirement(RegionRequirement::ClosestWithCrossRegionalOptions)?;
        read_locator.set_stats(DbLocatorStats::new());
        let read_pool = pool.bind(read_locator);

        Ok(Xdb {
            read_pool,
            read_stats: Arc::new(ConnectionStats::new("read connection".to_string())),
            write_pool,
            write_stats: Arc::new(ConnectionStats::new("write connection".to_string())),
        })
    }

    pub async fn write_conn(&self) -> Result<MysqlClientConnection> {
        let conn = self.write_pool.get_connection().await?;

        Ok(conn)
    }

    pub async fn write_conns(&self) -> Result<SqlConnections> {
        let write_connection: Connection =
            MysqlConnection::new(self.write_pool.clone(), self.write_stats.clone()).into();

        Ok(SqlConnections::new_single(write_connection))
    }

    pub async fn read_conn(&self) -> Result<MysqlClientConnection> {
        let conn = self.read_pool.get_connection().await?;

        Ok(conn)
    }

    pub async fn read_conns(&self) -> Result<SqlConnections> {
        let read_connection: Connection =
            MysqlConnection::new(self.read_pool.clone(), self.read_stats.clone()).into();

        Ok(SqlConnections::new_single(read_connection))
    }
}
