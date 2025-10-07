/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use blobstore::Blobstore;
use context::CoreContext;
use mononoke_types::Blob;
use mononoke_types::BlobstoreKey;
use mononoke_types::BlobstoreValue;
use mononoke_types::RedactionKeyList;
use mononoke_types::typed_hash::RedactionKeyListId;
use redactedblobstore::RedactionConfigBlobstore;
use redactedblobstore::load;

pub async fn create_key_list(
    ctx: &CoreContext,
    redaction_blobstore: &RedactionConfigBlobstore,
    keys: Vec<String>,
) -> Result<RedactionKeyListId> {
    let blob = RedactionKeyList { keys }.into_blob();
    let id = store(ctx, blob, redaction_blobstore).await?;

    println!("Redaction saved as: {}", id);
    println!(concat!(
        "To finish the redaction process, you need to commit this id to ",
        "scm/mononoke/redaction/redaction_sets.cconf in configerator"
    ));
    Ok(id)
}

pub async fn fetch_key_list(
    ctx: &CoreContext,
    redaction_blobstore: &RedactionConfigBlobstore,
    redaction_id: RedactionKeyListId,
) -> Result<RedactionKeyList> {
    Ok(load(ctx, redaction_id, redaction_blobstore).await?)
}

async fn store<'a>(
    ctx: &'a CoreContext,
    blob: Blob<RedactionKeyListId>,
    blobstore: &'a impl Blobstore,
) -> Result<RedactionKeyListId> {
    let bytes = blob.clone().into();
    let id = blob.id();
    blobstore.put(ctx, id.blobstore_key(), bytes).await?;
    Ok(*id)
}
