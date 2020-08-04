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
    MetadataDatabaseConfig, MultiplexId, MultiplexedStoreType, RemoteDatabaseConfig,
    RemoteMetadataDatabaseConfig, ShardableRemoteDatabaseConfig, ShardedRemoteDatabaseConfig,
    StorageConfig,
};
use nonzero_ext::nonzero;
use repos::{
    RawBlobstoreConfig, RawDbConfig, RawDbLocal, RawDbRemote, RawDbShardableRemote,
    RawDbShardedRemote, RawFilestoreParams, RawMetadataConfig, RawMultiplexedStoreType,
    RawStorageConfig,
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
            RawBlobstoreConfig::blob_files(raw) => BlobConfig::Files {
                path: PathBuf::from(raw.path),
            },
            RawBlobstoreConfig::blob_sqlite(raw) => BlobConfig::Sqlite {
                path: PathBuf::from(raw.path),
            },
            RawBlobstoreConfig::manifold(raw) => BlobConfig::Manifold {
                bucket: raw.manifold_bucket,
                prefix: raw.manifold_prefix,
            },
            RawBlobstoreConfig::mysql(raw) => BlobConfig::Mysql {
                remote: raw.remote.convert()?,
            },
            RawBlobstoreConfig::multiplexed(raw) => {
                let unchecked_minimum_successful_writes: usize =
                    raw.minimum_successful_writes.unwrap_or(1).try_into()?;

                if unchecked_minimum_successful_writes > raw.components.len() {
                    return Err(anyhow!(
                        "Not enough blobstores for {} required writes (have {})",
                        unchecked_minimum_successful_writes,
                        raw.components.len()
                    ));
                }

                let minimum_successful_writes =
                    NonZeroUsize::new(unchecked_minimum_successful_writes).ok_or_else(|| {
                        anyhow!("Must require at least 1 successful write to make a put succeed")
                    })?;

                BlobConfig::Multiplexed {
                    multiplex_id: raw
                        .multiplex_id
                        .map(MultiplexId::new)
                        .ok_or_else(|| anyhow!("missing multiplex_id from configuration"))?,
                    scuba_table: raw.scuba_table,
                    scuba_sample_rate: parse_scuba_sample_rate(raw.scuba_sample_rate)?,
                    blobstores: raw
                        .components
                        .into_iter()
                        .map(|comp| {
                            Ok((
                                BlobstoreId::new(comp.blobstore_id.try_into()?),
                                comp.store_type
                                    .convert()?
                                    .unwrap_or(MultiplexedStoreType::Normal),
                                comp.blobstore.convert()?,
                            ))
                        })
                        .collect::<Result<Vec<_>>>()?,
                    minimum_successful_writes,
                    queue_db: raw
                        .queue_db
                        .ok_or_else(|| anyhow!("missing queue_db from configuration"))?
                        .convert()?,
                }
            }
            RawBlobstoreConfig::manifold_with_ttl(raw) => {
                let ttl = Duration::from_secs(raw.ttl_secs.try_into()?);
                BlobConfig::ManifoldWithTtl {
                    bucket: raw.manifold_bucket,
                    prefix: raw.manifold_prefix,
                    ttl,
                }
            }
            RawBlobstoreConfig::logging(raw) => BlobConfig::Logging {
                scuba_table: raw.scuba_table,
                scuba_sample_rate: parse_scuba_sample_rate(raw.scuba_sample_rate)?,
                blobconfig: Box::new(raw.blobstore.convert()?),
            },
            RawBlobstoreConfig::pack(raw) => BlobConfig::Pack {
                blobconfig: Box::new(raw.blobstore.convert()?),
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
            RawDbShardableRemote::unsharded(raw) => {
                Ok(ShardableRemoteDatabaseConfig::Unsharded(raw.convert()?))
            }
            RawDbShardableRemote::sharded(raw) => {
                Ok(ShardableRemoteDatabaseConfig::Sharded(raw.convert()?))
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
            RawDbConfig::local(raw) => Ok(DatabaseConfig::Local(raw.convert()?)),
            RawDbConfig::remote(raw) => Ok(DatabaseConfig::Remote(raw.convert()?)),
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
            RawMetadataConfig::local(raw) => Ok(MetadataDatabaseConfig::Local(raw.convert()?)),
            RawMetadataConfig::remote(raw) => Ok(MetadataDatabaseConfig::Remote(
                RemoteMetadataDatabaseConfig {
                    primary: raw.primary.convert()?,
                    filenodes: raw.filenodes.convert()?,
                    mutation: raw.mutation.convert()?,
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

impl Convert for RawMultiplexedStoreType {
    type Output = MultiplexedStoreType;

    fn convert(self) -> Result<Self::Output> {
        match self {
            RawMultiplexedStoreType::normal(_) => Ok(MultiplexedStoreType::Normal),
            RawMultiplexedStoreType::write_mostly(_) => Ok(MultiplexedStoreType::WriteMostly),
            RawMultiplexedStoreType::UnknownField(field) => {
                Err(anyhow!("unknown store type {}", field))
            }
        }
    }
}
