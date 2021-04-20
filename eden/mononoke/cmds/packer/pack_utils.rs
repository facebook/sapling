/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Result};
use blobstore::{Blobstore, BlobstoreBytes, BlobstoreWithLink};
use context::CoreContext;
use futures::stream::{FuturesUnordered, TryStreamExt};
use packblob::{EmptyPack, Pack, PackBlob};

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

// Creates a pack from blobs in keys
async fn create_pack<T: BlobstoreWithLink>(
    ctx: &CoreContext,
    blobstore: &PackBlob<T>,
    zstd_level: i32,
    repo_prefix: &str,
    keys: &[&str],
) -> Result<Pack> {
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

    let mut blobs: Vec<_> = blob_fetches.try_collect().await?;
    let mut best_pack = None;

    // TODO: Rewrite as a try_for_each_concurrent or a try_fold with spawning.
    for _ in 0..blobs.len() {
        let pack = tokio::task::spawn_blocking({
            let blobs = blobs.clone();
            move || try_pack(zstd_level, blobs)
        })
        .await??;
        if is_better_pack(best_pack.as_ref(), &pack) {
            best_pack = Some(pack);
        }
        blobs.rotate_left(1);
    }

    best_pack.ok_or_else(|| anyhow!("Did not succeed in finding a good pack"))
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
    let pack = create_pack(ctx, blobstore, zstd_level, repo_prefix, keys).await?;
    if !dry_run {
        blobstore
            .put_packed(ctx, pack, repo_prefix.to_string(), pack_prefix.to_string())
            .await?;
    }
    Ok(())
}
