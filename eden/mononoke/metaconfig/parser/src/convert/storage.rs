/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Result};
use metaconfig_types::{
    BlobConfig, BlobstoreId, DatabaseConfig, FilestoreParams, LocalDatabaseConfig,
    MetadataDatabaseConfig, MultiplexId, RemoteDatabaseConfig, RemoteMetadataDatabaseConfig,
    ShardableRemoteDatabaseConfig, ShardedRemoteDatabaseConfig, StorageConfig,
};
use nonzero_ext::nonzero;
use repos::{
    RawBlobstoreConfig, RawDbConfig, RawDbLocal, RawDbRemote, RawDbShardableRemote,
    RawDbShardedRemote, RawFilestoreParams, RawMetadataConfig, RawStorageConfig,
};

use crate::convert::Convert;

impl Convert for RawStorageConfig {
    type Output = StorageConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(StorageConfig {
            metadata: self.metadata.convert()?,
            blobstore: self.blobstore.convert()?,
        })
    }
}

impl Convert for RawBlobstoreConfig {
    type Output = BlobConfig;

    fn convert(self) -> Result<Self::Output> {
        let config = match self {
            RawBlobstoreConfig::disabled(_) => BlobConfig::Disabled,
            RawBlobstoreConfig::blob_files(def) => BlobConfig::Files {
                path: PathBuf::from(def.path),
            },
            RawBlobstoreConfig::blob_sqlite(def) => BlobConfig::Sqlite {
                path: PathBuf::from(def.path),
            },
            RawBlobstoreConfig::manifold(def) => BlobConfig::Manifold {
                bucket: def.manifold_bucket,
                prefix: def.manifold_prefix,
            },
            RawBlobstoreConfig::mysql(def) => {
                if let Some(remote) = def.remote {
                    BlobConfig::Mysql {
                        remote: remote.convert()?,
                    }
                } else {
                    BlobConfig::Mysql {
                        remote: ShardableRemoteDatabaseConfig::Sharded(
                            ShardedRemoteDatabaseConfig {
                                shard_map: def.mysql_shardmap.ok_or_else(|| anyhow!("mysql shard name must be specified"))?,
                                shard_num: NonZeroUsize::new(def.mysql_shard_num.ok_or_else(|| anyhow!("mysql shard num must be specified"))?.try_into()?)
                                    .ok_or_else(|| anyhow!("mysql shard num must be specified and an integer larger than 0"))?,
                            },
                        ),
                    }
                }
            }
            RawBlobstoreConfig::multiplexed(def) => BlobConfig::Multiplexed {
                multiplex_id: def
                    .multiplex_id
                    .map(MultiplexId::new)
                    .ok_or_else(|| anyhow!("missing multiplex_id from configuration"))?,
                scuba_table: def.scuba_table,
                scuba_sample_rate: parse_scuba_sample_rate(def.scuba_sample_rate)?,
                blobstores: def
                    .components
                    .into_iter()
                    .map(|comp| {
                        Ok((
                            BlobstoreId::new(comp.blobstore_id.try_into()?),
                            comp.blobstore.convert()?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?,
                queue_db: def
                    .queue_db
                    .ok_or_else(|| anyhow!("missing queue_db from configuration"))?
                    .convert()?,
            },
            RawBlobstoreConfig::manifold_with_ttl(def) => {
                let ttl = Duration::from_secs(def.ttl_secs.try_into()?);
                BlobConfig::ManifoldWithTtl {
                    bucket: def.manifold_bucket,
                    prefix: def.manifold_prefix,
                    ttl,
                }
            }
            RawBlobstoreConfig::logging(def) => BlobConfig::Logging {
                scuba_table: def.scuba_table,
                scuba_sample_rate: parse_scuba_sample_rate(def.scuba_sample_rate)?,
                blobconfig: Box::new(def.blobstore.convert()?),
            },
            RawBlobstoreConfig::UnknownField(f) => {
                return Err(anyhow!("unsupported blobstore configuration ({})", f));
            }
        };
        Ok(config)
    }
}

fn parse_scuba_sample_rate(sample_rate: Option<i64>) -> Result<NonZeroU64> {
    let rate = sample_rate
        .map(|rate| {
            NonZeroU64::new(rate.try_into()?)
                .ok_or_else(|| anyhow!("scuba_sample_rate must be an integer larger than zero"))
        })
        .transpose()?
        .unwrap_or(nonzero!(100_u64));

    Ok(rate)
}

impl Convert for RawDbLocal {
    type Output = LocalDatabaseConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(LocalDatabaseConfig {
            path: PathBuf::from(self.local_db_path),
        })
    }
}

impl Convert for RawDbRemote {
    type Output = RemoteDatabaseConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(RemoteDatabaseConfig {
            db_address: self.db_address,
        })
    }
}

impl Convert for RawDbShardedRemote {
    type Output = ShardedRemoteDatabaseConfig;

    fn convert(self) -> Result<Self::Output> {
        let shard_num = NonZeroUsize::new(self.shard_num.try_into()?)
            .ok_or_else(|| anyhow!("sharded remote shard_num must be > 0"))?;

        Ok(ShardedRemoteDatabaseConfig {
            shard_map: self.shard_map,
            shard_num,
        })
    }
}

impl Convert for RawDbShardableRemote {
    type Output = ShardableRemoteDatabaseConfig;

    fn convert(self) -> Result<Self::Output> {
        match self {
            RawDbShardableRemote::unsharded(def) => {
                Ok(ShardableRemoteDatabaseConfig::Unsharded(def.convert()?))
            }
            RawDbShardableRemote::sharded(def) => {
                Ok(ShardableRemoteDatabaseConfig::Sharded(def.convert()?))
            }
            RawDbShardableRemote::UnknownField(f) => {
                Err(anyhow!("unsupported database configuration ({})", f))
            }
        }
    }
}

impl Convert for RawDbConfig {
    type Output = DatabaseConfig;

    fn convert(self) -> Result<Self::Output> {
        match self {
            RawDbConfig::local(def) => Ok(DatabaseConfig::Local(def.convert()?)),
            RawDbConfig::remote(def) => Ok(DatabaseConfig::Remote(def.convert()?)),
            RawDbConfig::UnknownField(f) => {
                Err(anyhow!("unsupported database configuration ({})", f))
            }
        }
    }
}

impl Convert for RawMetadataConfig {
    type Output = MetadataDatabaseConfig;

    fn convert(self) -> Result<Self::Output> {
        match self {
            RawMetadataConfig::local(def) => Ok(MetadataDatabaseConfig::Local(def.convert()?)),
            RawMetadataConfig::remote(def) => Ok(MetadataDatabaseConfig::Remote(
                RemoteMetadataDatabaseConfig {
                    primary: def.primary.convert()?,
                    filenodes: def.filenodes.convert()?,
                    mutation: def.mutation.convert()?,
                },
            )),
            RawMetadataConfig::UnknownField(f) => Err(anyhow!(
                "unsupported metadata database configuration ({})",
                f
            )),
        }
    }
}

impl Convert for RawFilestoreParams {
    type Output = FilestoreParams;

    fn convert(self) -> Result<Self::Output> {
        Ok(FilestoreParams {
            chunk_size: self.chunk_size.try_into()?,
            concurrency: self.concurrency.try_into()?,
        })
    }
}
