/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::BlobstoreUnlinkOps;
use context::CoreContext;
use futures::future::FutureExt;
use futures::stream::FuturesUnordered;
use futures::stream::TryStreamExt;
use packblob::get_entry_compressed_size;
use packblob::EmptyPack;
use packblob::Pack;
use packblob::PackBlob;
use packblob::SingleCompressed;
use retry::retry_always;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;
use tokio::task::spawn_blocking;

type BlobsWithKeys = Vec<(String, BlobstoreBytes)>;

const BLOBSTORE_KEY: &str = "blobstore_key";
const COMPRESSED_SIZE: &str = "compressed_size";
const PACK_KEY: &str = "pack_key";
const UNCOMPRESSED_SIZE: &str = "uncompressed_size";
const BASE_RETRY_DELAY_MS: u64 = 2000;
const RETRIES: usize = 10;

// Tries to pack with the first blob from `blobs` as the dictionary for the other blobs
fn try_pack(zstd_level: i32, blobs: Vec<(String, BlobstoreBytes)>) -> Result<Pack> {
    let empty_pack = EmptyPack::new(zstd_level);

    let mut blobs = blobs.into_iter();

    let (dict_key, dict_blob) = blobs.next().ok_or_else(|| anyhow!("No blobs to pack"))?;
    let mut pack = empty_pack.add_base_blob(dict_key.clone(), dict_blob)?;
    for (key, blob) in blobs {
        pack.add_delta_blob(dict_key.clone(), key, blob)?;
    }

    Ok(pack)
}

struct PackContainer {
    pack: Option<Pack>,
    sizes: Vec<usize>,
    best_size_so_far: usize,
}

impl PackContainer {
    pub fn default() -> PackContainer {
        PackContainer {
            pack: None,
            sizes: vec![],
            best_size_so_far: usize::MAX,
        }
    }
}

async fn find_best_pack(
    mut blobs: BlobsWithKeys,
    zstd_level: i32,
    container: PackContainer,
) -> Result<PackContainer> {
    blobs.sort_by(|a, b| b.1.len().cmp(&a.1.len()));

    let build_packs = FuturesUnordered::new();
    build_packs.push({
        let blobs = blobs.clone();
        async { tokio::task::spawn_blocking(move || try_pack(zstd_level, blobs)).await? }
    });

    let container = build_packs
        .try_fold(container, |mut acc, new| async move {
            let new_size = new.get_compressed_size().unwrap();
            acc.sizes.push(new_size);
            if acc.best_size_so_far > new_size {
                acc.best_size_so_far = new_size;
                acc.pack = Some(new);
                Ok(acc)
            } else {
                // do nothing
                Ok(acc)
            }
        })
        .await?;
    Ok(container)
}

async fn fetch_blobs<T: BlobstoreUnlinkOps>(
    ctx: &CoreContext,
    blobstore: &PackBlob<T>,
    repo_prefix: &str,
    keys: &[&str],
) -> Result<BlobsWithKeys> {
    let blob_fetches: FuturesUnordered<_> = keys
        .iter()
        .map(|key| async move {
            let blob = blobstore
                .get(ctx, key)
                .await?
                .ok_or_else(|| anyhow!("Blob {} not in store", key))?
                .into_bytes();
            let pack_key = key
                .strip_prefix(repo_prefix)
                .ok_or_else(|| anyhow!("Could not strip {} from {}", repo_prefix, key))?;
            Result::<_>::Ok((pack_key.to_string(), blob))
        })
        .collect();

    blob_fetches.try_collect().await
}

/// Given a list of keys to repack, convert them to a single pack with retries
pub async fn repack_keys_with_retry<T: BlobstoreUnlinkOps>(
    ctx: &CoreContext,
    blobstore: &PackBlob<T>,
    pack_prefix: &str,
    zstd_level: i32,
    repo_prefix: &str,
    keys: &[&str],
    dry_run: bool,
    scuba: &MononokeScubaSampleBuilder,
    tuning_info_scuba: &MononokeScubaSampleBuilder,
    logger: &Logger,
) -> Result<()> {
    let _ = retry_always(
        logger,
        |_| {
            repack_keys(
                ctx,
                blobstore,
                pack_prefix,
                zstd_level,
                repo_prefix,
                keys,
                dry_run,
                scuba,
                tuning_info_scuba,
            )
        },
        BASE_RETRY_DELAY_MS,
        RETRIES,
    )
    .await?;
    Ok(())
}

/// Given a list of keys to repack, convert them to a single pack
pub async fn repack_keys<T: BlobstoreUnlinkOps>(
    ctx: &CoreContext,
    blobstore: &PackBlob<T>,
    pack_prefix: &str,
    zstd_level: i32,
    repo_prefix: &str,
    keys: &[&str],
    dry_run: bool,
    scuba: &MononokeScubaSampleBuilder,
    tuning_info_scuba: &MononokeScubaSampleBuilder,
) -> Result<()> {
    let mut tuning_scuba = tuning_info_scuba.clone();

    let mut last_event_time = Instant::now();
    let blobs = fetch_blobs(ctx, blobstore, repo_prefix, keys).await?;
    let mut elapsed = last_event_time.elapsed();
    let mut elapsed_in_s = elapsed.as_secs_f64();
    tuning_scuba.add_opt("pack_length", Some(blobs.len()));
    tuning_scuba.add_opt("blobs_download_time", Some(elapsed_in_s));

    // Compress blobs individually
    let compression_futs: FuturesUnordered<_> = blobs
        .clone()
        .into_iter()
        .map(|(key, blob)| async move {
            let uncompressed_size = blob.as_bytes().len();
            let single = spawn_blocking(move || SingleCompressed::new(zstd_level, blob)).await??;
            Result::<_, Error>::Ok((key, uncompressed_size, single))
        })
        .collect();

    last_event_time = Instant::now();
    let single_compressed: Vec<_> = compression_futs.try_collect().await?;
    elapsed = last_event_time.elapsed();
    elapsed_in_s = elapsed.as_secs_f64();
    tuning_scuba.add_opt("compressing_blobs_invidivually_time", Some(elapsed_in_s));

    // Find the best packing strategy
    last_event_time = Instant::now();
    let pack = if keys.len() > 1 {
        let container = find_best_pack(blobs, zstd_level, PackContainer::default()).await?;
        let sizes_str = container
            .sizes
            .into_iter()
            .map(|i| i.to_string())
            .collect::<Vec<String>>()
            .join(",");
        tuning_scuba.add_opt("possible_pack_sizes", Some(sizes_str));
        container.pack
    } else {
        None
    };
    elapsed = last_event_time.elapsed();
    elapsed_in_s = elapsed.as_secs_f64();
    tuning_scuba.add_opt("finding_best_packing_strategy_time", Some(elapsed_in_s));

    let mut uncompressed_sizes = HashMap::new();
    let single_compressed_size =
        single_compressed
            .iter()
            .try_fold(0usize, |size, (key, uncompressed_size, item)| {
                uncompressed_sizes.insert(key, *uncompressed_size);
                Ok::<_, Error>(size + item.get_compressed_size()?)
            })?;

    let total_uncompressed_size: usize = uncompressed_sizes
        .values()
        .cloned()
        .collect::<Vec<usize>>()
        .iter()
        .sum();
    tuning_scuba.add_opt("uncompressed_size", Some(total_uncompressed_size));

    match pack {
        Some(pack) if pack.get_compressed_size()? < single_compressed_size => {
            let pack_size = pack.get_compressed_size().unwrap() as f64;
            let single_size = single_compressed_size as f64;
            tuning_scuba.add_opt("packed_size", Some(pack_size));
            tuning_scuba.add_opt("single_compressed_size", Some(single_size));
            if !dry_run {
                // gather info for logs
                let logs: Vec<MononokeScubaSampleBuilder> = pack
                    .entries()
                    .iter()
                    .map(|e| {
                        get_entry_compressed_size(e).map(|compressed_size| {
                            let mut scuba = scuba.clone();
                            scuba.add(BLOBSTORE_KEY, format!("{}{}", repo_prefix, e.key));
                            scuba.add_opt(
                                UNCOMPRESSED_SIZE,
                                uncompressed_sizes.get(&e.key).copied(),
                            );
                            scuba.add(COMPRESSED_SIZE, compressed_size);
                            scuba
                        })
                    })
                    .collect::<Result<Vec<MononokeScubaSampleBuilder>>>()?;

                // store
                let pack_key = blobstore
                    .put_packed(ctx, pack, repo_prefix.to_string(), pack_prefix.to_string())
                    .await?;

                // log what we stored
                tuning_scuba.add_opt("packed_key", Some(pack_key.as_str()));
                for mut scuba in logs {
                    scuba.add(PACK_KEY, pack_key.as_str());
                    scuba.log();
                }
            }
        }
        Some(_) | None => {
            if !dry_run {
                let put_futs: FuturesUnordered<_> = single_compressed
                    .into_iter()
                    .map(|(key, uncompressed_size, value)| {
                        let key = format!("{}{}", repo_prefix, key);
                        let mut scuba = scuba.clone();
                        scuba.add(BLOBSTORE_KEY, key.as_str());
                        scuba.add(UNCOMPRESSED_SIZE, uncompressed_size);
                        let compressed_size = value.get_compressed_size();
                        blobstore.put_single(ctx, key, value).map(move |v| {
                            scuba.add(COMPRESSED_SIZE, compressed_size?);
                            scuba.log();
                            v
                        })
                    })
                    .collect();
                put_futs.try_for_each(|_| async { Ok(()) }).await?;
            }
        }
    }
    tuning_scuba.log();
    Ok(())
}
