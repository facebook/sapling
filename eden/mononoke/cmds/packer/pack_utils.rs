/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Error, Result};
use blobstore::{Blobstore, BlobstoreBytes, BlobstoreWithLink};
use context::CoreContext;
use futures::stream::{FuturesUnordered, TryStreamExt};
use packblob::{EmptyPack, Pack, PackBlob, SingleCompressed};
use tokio::task::spawn_blocking;

type BlobsWithKeys = Vec<(String, BlobstoreBytes)>;

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

fn is_better_pack(best_so_far: Option<&Pack>, new: &Pack) -> bool {
    if let Some(best_so_far) = best_so_far {
        best_so_far.get_compressed_size() > new.get_compressed_size()
    } else {
        true
    }
}

async fn find_best_pack(mut blobs: BlobsWithKeys, zstd_level: i32) -> Result<Option<Pack>> {
    let build_packs = FuturesUnordered::new();
    for _ in 0..blobs.len() {
        build_packs.push({
            let blobs = blobs.clone();
            async { tokio::task::spawn_blocking({ move || try_pack(zstd_level, blobs) }).await? }
        });
        blobs.rotate_left(1);
    }

    build_packs
        .try_fold(None, |best, new| async move {
            if is_better_pack(best.as_ref(), &new) {
                Ok(Some(new))
            } else {
                Ok(best)
            }
        })
        .await
}

async fn fetch_blobs<T: BlobstoreWithLink>(
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
pub async fn repack_keys<T: BlobstoreWithLink>(
    ctx: &CoreContext,
    blobstore: &PackBlob<T>,
    pack_prefix: &str,
    zstd_level: i32,
    repo_prefix: &str,
    keys: &[&str],
    dry_run: bool,
) -> Result<()> {
    let blobs = fetch_blobs(ctx, blobstore, repo_prefix, keys).await?;
    let compression_futs: FuturesUnordered<_> = blobs
        .clone()
        .into_iter()
        .map(|(key, blob)| async move {
            let single = spawn_blocking(move || SingleCompressed::new(zstd_level, blob)).await??;
            Result::<_, Error>::Ok((key, single))
        })
        .collect();

    let single_compressed: Vec<_> = compression_futs.try_collect().await?;
    let pack = if keys.len() > 1 {
        find_best_pack(blobs, zstd_level).await?
    } else {
        None
    };

    let single_compressed_size = single_compressed
        .iter()
        .fold(0usize, |size, (_, item)| size + item.get_compressed_size());
    if !dry_run {
        match pack {
            Some(pack) if pack.get_compressed_size() < single_compressed_size => {
                blobstore
                    .put_packed(ctx, pack, repo_prefix.to_string(), pack_prefix.to_string())
                    .await?;
            }
            Some(_) | None => {
                let put_futs: FuturesUnordered<_> = single_compressed
                    .into_iter()
                    .map(|(key, value)| {
                        let key = format!("{}{}", repo_prefix, key);
                        blobstore.put_single(ctx, key, value)
                    })
                    .collect();
                put_futs.try_for_each(|_| async { Ok(()) }).await?;
            }
        }
    }
    Ok(())
}
