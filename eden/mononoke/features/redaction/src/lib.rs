/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::File;
use std::io::Write;
use std::path::Path;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use blobstore::Storable;
use context::CoreContext;
use futures::future::try_join;
use mononoke_app::MononokeApp;
use mononoke_types::BlobstoreValue;
use mononoke_types::RedactionKeyList;

pub async fn create_key_list(
    ctx: &CoreContext,
    app: &MononokeApp,
    keys: Vec<String>,
    output_file: Option<&Path>,
) -> Result<()> {
    let redaction_blobstore = app.redaction_config_blobstore().await?;
    let darkstorm_blobstore = app.redaction_config_blobstore_for_darkstorm().await?;

    let blob = RedactionKeyList { keys }.into_blob();
    let (id1, id2) = try_join(
        blob.clone().store(ctx, &redaction_blobstore),
        blob.store(ctx, &darkstorm_blobstore),
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
    if let Some(output_file) = output_file {
        let mut output = File::create(output_file).with_context(|| {
            format!(
                "Failed to open output file '{}'",
                output_file.to_string_lossy()
            )
        })?;
        output
            .write_all(id1.to_string().as_bytes())
            .with_context(|| {
                format!(
                    "Failed to write to output file '{}'",
                    output_file.to_string_lossy()
                )
            })?;
    }
    Ok(())
}
