/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Context;
use async_trait::async_trait;
use bytes::Bytes;
use configerator_client::AsyncConfigeratorClient;
use configerator_client::AsyncConfigeratorClientExt;
use ephemeral_shards::EphemeralShardGuard;
use ephemeral_shards::ShardTTL;
use fbinit::FacebookInit;
use futures::stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;
use if_aosc_srclients::AoscServiceClient;
use mysql_client::query;
use mysql_client::BoundConnectionFactory;
use mysql_client::ConnectionPool;
use mysql_client::ConnectionPoolOptionsBuilder;
use mysql_client::DbLocator;
use mysql_client::InstanceRequirement;
use mysql_client::MysqlCppClient;
use table_schema::DatabaseSchemas;
use table_schema::Domain;
use table_schema::DomainType;
use table_schema::TableSchema;

pub type ConfigeratorClient =
    Arc<dyn AsyncConfigeratorClient<RawConfig = Bytes> + Send + Sync + 'static>;
pub type AoscClient = AoscServiceClient;

const ONCALL: &str = "scm_server_infra";
const CONFIG_TIMEOUT: Duration = Duration::from_secs(30);
const READ_WRITE_DB_ROLE: &str = "scriptrw";
const SCHEMA_FILENAME: &str = ".sql_schema_domains";
const TABLE_SCHEMA_SUFFIX: &str = ".sql_table_schema";

#[async_trait]
trait AoscClientExt {
    async fn shard_map_for_shard(&self, shard: &str) -> anyhow::Result<Option<String>>;
    async fn config_paths_for_shard(&self, shard: &str) -> anyhow::Result<Vec<String>>;
}

#[async_trait]
impl AoscClientExt for AoscClient {
    async fn shard_map_for_shard(&self, shard: &str) -> anyhow::Result<Option<String>> {
        let check_exists = true;
        let mut res = self
            .get_schema_domains_for_shards(&[shard.to_owned()], check_exists)
            .await?;
        let domains = res
            .remove(shard)
            .ok_or_else(|| anyhow!("Expected \"{shard}\" in domain response: `{res:?}`"))?;
        let maybe_shard_map = domains
            .into_iter()
            .find(|domain| domain.r#type == DomainType::SHARDMAP)
            .map(|domain| domain.name);
        Ok(maybe_shard_map)
    }

    async fn config_paths_for_shard(&self, shard: &str) -> anyhow::Result<Vec<String>> {
        let maybe_shard_map = self.shard_map_for_shard(shard).await?;
        let domain = match maybe_shard_map {
            Some(shard_map) => Domain {
                r#type: DomainType::SHARDMAP,
                name: shard_map,
                ..Default::default()
            },
            None => Domain {
                r#type: DomainType::SHARD,
                name: shard.to_string(),
                ..Default::default()
            },
        };
        let check_exists = true;
        let res = self.config_paths_for_domain(&domain, check_exists).await?;
        Ok(res)
    }
}

async fn fetch_table_schemas(
    shard_name: &str,
    configerator: &ConfigeratorClient,
    aosc: &AoscClient,
) -> anyhow::Result<Vec<TableSchema>> {
    let config_paths = aosc.config_paths_for_shard(shard_name).await?;
    let tables: Vec<_> = stream::iter(config_paths)
        .flat_map(move |config_path| {
            async move {
                let schemas_path = format!("{config_path}/{SCHEMA_FILENAME}");
                let schemas: DatabaseSchemas = configerator
                    .get_parsed_config_async(&schemas_path, Some(CONFIG_TIMEOUT))
                    .await?;
                let tables = schemas
                    .tables
                    .into_keys()
                    .map(move |table_name| format!("{config_path}/{table_name}"));
                Ok(stream::iter(tables.map(anyhow::Ok)))
            }
            .try_flatten_stream()
        })
        .map_ok(move |table| {
            async move {
                let table_path = format!("{table}{TABLE_SCHEMA_SUFFIX}");
                let schema: TableSchema = configerator
                    .get_parsed_config_async(&table_path, Some(CONFIG_TIMEOUT))
                    .await?;
                Ok(stream::once(async { anyhow::Ok(schema) }))
            }
            .try_flatten_stream()
        })
        // The box is to work around a rustc bug where the compiler can't tell
        // that the future is `Send`: https://fburl.com/7frg57kg
        .boxed()
        .try_flatten()
        .try_collect()
        .await?;
    Ok(tables)
}

#[derive(Clone)]
pub struct EphemeralShard {
    guard: Arc<EphemeralShardGuard>,
}

impl std::fmt::Display for EphemeralShard {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.name().fmt(f)
    }
}

#[derive(Clone)]
pub struct ConfigeratorSchemaClient {
    configerator_client: ConfigeratorClient,
    aosc_client: AoscClient,
}
impl ConfigeratorSchemaClient {
    pub fn new(configerator: ConfigeratorClient, aosc: AoscClient) -> Self {
        Self {
            configerator_client: configerator,
            aosc_client: aosc,
        }
    }
}

#[derive(Clone)]
pub enum EphemeralSchema {
    Configerator(ConfigeratorSchemaClient),
    Live,
}
impl EphemeralShard {
    pub async fn new(
        fb: FacebookInit,
        source_shard: Option<&str>,
        schema_source: EphemeralSchema,
    ) -> anyhow::Result<Self> {
        let source_shard_name = match schema_source {
            EphemeralSchema::Configerator(_) => None,
            EphemeralSchema::Live => source_shard.map(|s| s.to_string()),
        };

        let ephemeral_shard = EphemeralShardGuard::acquire_mysql8(
            fb,
            ONCALL,
            ShardTTL::ONE_DAY,
            source_shard_name, // Table schemas are manually set up below for configerator schema
            false,             // use_rocksdb
        )
        .await?;
        match schema_source {
            EphemeralSchema::Configerator(schema_client) => {
                if let Some(source_shard) = source_shard {
                    let tables = fetch_table_schemas(
                        source_shard,
                        &schema_client.configerator_client,
                        &schema_client.aosc_client,
                    )
                    .await?;
                    let client =
                        MysqlCppClient::new(fb).context("Failed to create MySQL C++ client")?;
                    let pool_options = ConnectionPoolOptionsBuilder::default()
                        .build()
                        .map_err(anyhow::Error::msg)?;
                    let pool = ConnectionPool::new(&client, &pool_options)?;
                    let write_locator = DbLocator::new_with_rolename(
                        ephemeral_shard.name(),
                        InstanceRequirement::Master,
                        READ_WRITE_DB_ROLE,
                    )
                    .context("Failed to create DbLocator")?;
                    let write_pool = pool.clone().bind(write_locator);
                    let mut conn = write_pool.get_connection().await?;
                    conn.multi_query(tables.iter().map(|table| query!(table.sql)))
                        .await
                        .context("Failed to create tables in ephemeral shard")?;
                }
            }
            EphemeralSchema::Live => {
                // The table schemas are automatically set up based on the source shard name
            }
        }

        Ok(Self {
            guard: Arc::new(ephemeral_shard),
        })
    }

    pub fn name(&self) -> &str {
        self.guard.name()
    }
}
