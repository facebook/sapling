/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU64;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use metaconfig_types::BlobConfig;
use metaconfig_types::BlobstoreId;
use metaconfig_types::BubbleDeletionMode;
use metaconfig_types::DatabaseConfig;
use metaconfig_types::EphemeralBlobstoreConfig;
use metaconfig_types::FilestoreParams;
use metaconfig_types::LocalDatabaseConfig;
use metaconfig_types::MetadataDatabaseConfig;
use metaconfig_types::MultiplexId;
use metaconfig_types::MultiplexedStoreType;
use metaconfig_types::PackConfig;
use metaconfig_types::PackFormat;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use metaconfig_types::ShardableRemoteDatabaseConfig;
use metaconfig_types::ShardedDatabaseConfig;
use metaconfig_types::ShardedRemoteDatabaseConfig;
use metaconfig_types::StorageConfig;
use nonzero_ext::nonzero;
use repos::RawBlobstoreConfig;
use repos::RawBlobstoreMultiplexedWal;
use repos::RawBlobstorePackConfig;
use repos::RawBlobstorePackFormat;
use repos::RawBubbleDeletionMode;
use repos::RawDbConfig;
use repos::RawDbLocal;
use repos::RawDbRemote;
use repos::RawDbShardableRemote;
use repos::RawDbShardedRemote;
use repos::RawEphemeralBlobstoreConfig;
use repos::RawFilestoreParams;
use repos::RawMetadataConfig;
use repos::RawMultiplexedStoreNormal;
use repos::RawMultiplexedStoreType;
use repos::RawMultiplexedStoreWriteMostly;
use repos::RawMultiplexedStoreWriteOnly;
use repos::RawShardedDbConfig;
use repos::RawStorageConfig;

use crate::convert::Convert;

impl Convert for RawStorageConfig {
    type Output = StorageConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(StorageConfig {
            metadata: self.metadata.convert()?,
            blobstore: self.blobstore.convert()?,
            ephemeral_blobstore: self
                .ephemeral_blobstore
                .map(RawEphemeralBlobstoreConfig::convert)
                .transpose()?,
        })
    }
}

impl Convert for RawBubbleDeletionMode {
    type Output = BubbleDeletionMode;

    fn convert(self) -> Result<Self::Output> {
        let deletion_mode = match self {
            RawBubbleDeletionMode::DISABLED => BubbleDeletionMode::Disabled,
            RawBubbleDeletionMode::MARK_ONLY => BubbleDeletionMode::MarkOnly,
            RawBubbleDeletionMode::MARK_AND_DELETE => BubbleDeletionMode::MarkAndDelete,
            v => return Err(anyhow!("Invalid value {} for enum BubbleDeletionMode", v)),
        };
        Ok(deletion_mode)
    }
}

impl Convert for RawEphemeralBlobstoreConfig {
    type Output = EphemeralBlobstoreConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(EphemeralBlobstoreConfig {
            metadata: self.metadata.convert()?,
            blobstore: self.blobstore.convert()?,
            initial_bubble_lifespan: Duration::from_secs(
                self.initial_bubble_lifespan_secs
                    .try_into()
                    .context("Failed to convert initial_bubble_lifespan")?,
            ),
            bubble_expiration_grace: Duration::from_secs(
                self.bubble_expiration_grace_secs
                    .try_into()
                    .context("Failed to convert bubble_expiration_grace")?,
            ),
            bubble_deletion_mode: self.bubble_deletion_mode.convert()?,
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
                let parse_quorum = |raw_value: i64, name: &'static str| {
                    let unchecked: usize = raw_value.try_into()?;

                    if unchecked > raw.components.len() {
                        return Err(anyhow!(
                            "Not enough blobstores for {} {} (have {})",
                            unchecked,
                            name,
                            raw.components.len()
                        ));
                    }

                    NonZeroUsize::new(unchecked)
                        .with_context(|| format!("Must require at least 1 {}", name))
                };

                let minimum_successful_writes =
                    parse_quorum(raw.minimum_successful_writes.unwrap_or(1), "minimum writes")?;
                let not_present_read_quorum = parse_quorum(
                    raw.not_present_read_quorum
                        .unwrap_or(raw.components.len().try_into()?),
                    "read quorum",
                )?;

                BlobConfig::Multiplexed {
                    multiplex_id: raw
                        .multiplex_id
                        .map(MultiplexId::new)
                        .ok_or_else(|| anyhow!("missing multiplex_id from configuration"))?,
                    scuba_table: raw.scuba_table,
                    multiplex_scuba_table: raw.multiplex_scuba_table,
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
                    not_present_read_quorum,
                    queue_db: raw
                        .queue_db
                        .ok_or_else(|| anyhow!("missing queue_db from configuration"))?
                        .convert()?,
                }
            }
            RawBlobstoreConfig::multiplexed_wal(RawBlobstoreMultiplexedWal {
                write_quorum,
                components,
                multiplex_id,
                queue_db,
                inner_blobstores_scuba_table,
                multiplex_scuba_table,
                scuba_sample_rate,
            }) => {
                let write_quorum: usize = write_quorum.try_into()?;
                if write_quorum > components.len() {
                    return Err(anyhow!(
                        "Not enough blobstores for {} write quorum (have {})",
                        write_quorum,
                        components.len()
                    ));
                }

                BlobConfig::MultiplexedWal {
                    multiplex_id: MultiplexId::new(multiplex_id),
                    blobstores: components
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
                    write_quorum,
                    queue_db: queue_db.convert()?,
                    inner_blobstores_scuba_table,
                    multiplex_scuba_table,
                    scuba_sample_rate: parse_scuba_sample_rate(scuba_sample_rate)?,
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
                pack_config: raw.pack_config.map(|c| c.convert()).transpose()?,
            },
            RawBlobstoreConfig::s3(raw) => BlobConfig::S3 {
                bucket: raw.bucket,
                keychain_group: raw.keychain_group,
                region_name: raw.region_name,
                endpoint: raw.endpoint,
                num_concurrent_operations: raw
                    .num_concurrent_operations
                    .map(|x| x.try_into())
                    .transpose()?,
                secret_name: raw.secret_name,
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

impl Convert for RawBlobstorePackFormat {
    type Output = PackFormat;

    fn convert(self) -> Result<Self::Output> {
        let pack_format = match self {
            RawBlobstorePackFormat::Raw(_) => PackFormat::Raw,
            RawBlobstorePackFormat::ZstdIndividual(zstd) => {
                PackFormat::ZstdIndividual(zstd.compression_level)
            }
            RawBlobstorePackFormat::UnknownField(f) => bail!("Unsupported PackFormat {}", f),
        };
        Ok(pack_format)
    }
}

impl Convert for RawBlobstorePackConfig {
    type Output = PackConfig;

    fn convert(self) -> Result<Self::Output> {
        let put_format = self.put_format.convert()?;
        Ok(PackConfig { put_format })
    }
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

impl Convert for RawShardedDbConfig {
    type Output = ShardedDatabaseConfig;

    fn convert(self) -> Result<Self::Output> {
        match self {
            RawShardedDbConfig::local(raw) => Ok(ShardedDatabaseConfig::Local(raw.convert()?)),
            RawShardedDbConfig::remote(raw) => Ok(ShardedDatabaseConfig::Remote(raw.convert()?)),
            RawShardedDbConfig::UnknownField(f) => {
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
                    sparse_profiles: raw.sparse_profiles.convert()?,
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
            RawMultiplexedStoreType::normal(RawMultiplexedStoreNormal {}) => {
                Ok(MultiplexedStoreType::Normal)
            }
            RawMultiplexedStoreType::write_only(RawMultiplexedStoreWriteOnly {}) => {
                Ok(MultiplexedStoreType::WriteOnly)
            }
            RawMultiplexedStoreType::write_mostly(RawMultiplexedStoreWriteMostly {}) => {
                Ok(MultiplexedStoreType::WriteOnly)
            }
            RawMultiplexedStoreType::UnknownField(field) => {
                Err(anyhow!("unknown store type {}", field))
            }
        }
    }
}
