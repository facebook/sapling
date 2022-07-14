/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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
use scuba_ext::MononokeScubaSampleBuilder;
use std::collections::HashMap;
use tokio::task::spawn_blocking;

type BlobsWithKeys = Vec<(String, BlobstoreBytes)>;

const BLOBSTORE_KEY: &str = "blobstore_key";
const COMPRESSED_SIZE: &str = "compressed_size";
const PACK_KEY: &str = "pack_key";
const UNCOMPRESSED_SIZE: &str = "uncompressed_size";

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

fn is_better_pack(best_so_far: Option<&Pack>, new: &Pack) -> Result<bool> {
    if let Some(best_so_far) = best_so_far {
        Ok(best_so_far.get_compressed_size()? > new.get_compressed_size()?)
    } else {
        Ok(true)
    }
}

async fn find_best_pack(mut blobs: BlobsWithKeys, zstd_level: i32) -> Result<Option<Pack>> {
    let build_packs = FuturesUnordered::new();
    for _ in 0..blobs.len() {
        build_packs.push({
            let blobs = blobs.clone();
            async { tokio::task::spawn_blocking(move || try_pack(zstd_level, blobs)).await? }
        });
        blobs.rotate_left(1);
    }

    build_packs
        .try_fold(None, |best, new| async move {
            if is_better_pack(best.as_ref(), &new)? {
                Ok(Some(new))
            } else {
                Ok(best)
            }
        })
        .await
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
) -> Result<()> {
    let blobs = fetch_blobs(ctx, blobstore, repo_prefix, keys).await?;
    let compression_futs: FuturesUnordered<_> = blobs
        .clone()
        .into_iter()
        .map(|(key, blob)| async move {
            let uncompressed_size = blob.as_bytes().len();
            let single = spawn_blocking(move || SingleCompressed::new(zstd_level, blob)).await??;
            Result::<_, Error>::Ok((key, uncompressed_size, single))
        })
        .collect();

    let mut uncompressed_sizes = HashMap::new();
    let single_compressed: Vec<_> = compression_futs.try_collect().await?;
    let pack = if keys.len() > 1 {
        find_best_pack(blobs, zstd_level).await?
    } else {
        None
    };

    let single_compressed_size =
        single_compressed
            .iter()
            .try_fold(0usize, |size, (key, uncompressed_size, item)| {
                uncompressed_sizes.insert(key, *uncompressed_size);
                Ok::<_, Error>(size + item.get_compressed_size()?)
            })?;
    if !dry_run {
        match pack {
            Some(pack) if pack.get_compressed_size()? < single_compressed_size => {
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
                for mut scuba in logs {
                    scuba.add(PACK_KEY, pack_key.as_str());
                    scuba.log();
                }
            }
            Some(_) | None => {
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
    Ok(())
}
