/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Result;
use blobstore::Loadable;
use blobstore::Storable;
use context::CoreContext;
use futures::future::try_join;
use mononoke_types::typed_hash::RedactionKeyListId;
use mononoke_types::BlobstoreValue;
use mononoke_types::RedactionKeyList;
use redactedblobstore::RedactionConfigBlobstore;

pub async fn create_key_list(
    ctx: &CoreContext,
    redaction_blobstore: &RedactionConfigBlobstore,
    darkstorm_blobstore: &RedactionConfigBlobstore,
    keys: Vec<String>,
) -> Result<RedactionKeyListId> {
    let blob = RedactionKeyList { keys }.into_blob();
    let (id1, id2) = try_join(
        blob.clone().store(ctx, redaction_blobstore),
        blob.store(ctx, darkstorm_blobstore),
    )
    .await?;
    if id1 != id2 {
        bail!(
            "Id mismatch on darkstorm and non-darkstorm blobstores: {} vs {}",
            id1,
            id2
        );
    }

    println!("Redaction saved as: {}", id1);
    println!(concat!(
        "To finish the redaction process, you need to commit this id to ",
        "scm/mononoke/redaction/redaction_sets.cconf in configerator"
    ));
    Ok(id1)
}

pub async fn fetch_key_list(
    ctx: &CoreContext,
    redaction_blobstore: &RedactionConfigBlobstore,
    redaction_id: RedactionKeyListId,
) -> Result<RedactionKeyList> {
    Ok(redaction_id.load(ctx, redaction_blobstore).await?)
}
