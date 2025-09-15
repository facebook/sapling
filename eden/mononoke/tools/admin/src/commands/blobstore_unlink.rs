/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use anyhow::Result;
use anyhow::format_err;
use clap::Parser;
use metaconfig_types::BlobConfig;
use metaconfig_types::BlobstoreId;
use mononoke_app::MononokeApp;
use mononoke_app::args::AsRepoArg;
use mononoke_app::args::RepoArgs;

/// Unlink blobstore keys
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    /// If the repo's blobstore is multiplexed and we don't need to unlink from all underlying
    /// blobstores, pass this inner blobstore id. We shall only unlink from that particular
    /// underlying blobstore.
    #[clap(long)]
    inner_blobstore_id: Option<u64>,

    /// Key of the blob to unlink
    key: String,
}

fn remove_wrapper_blobconfigs(mut blob_config: BlobConfig) -> BlobConfig {
    // Pack is a wrapper store - remove it
    while let BlobConfig::Pack { ref blobconfig, .. } = blob_config {
        blob_config = BlobConfig::clone(blobconfig);
    }
    blob_config
}

pub fn get_blobconfig(
    blob_config: BlobConfig,
    inner_blobstore_id: Option<u64>,
) -> Result<BlobConfig> {
    match inner_blobstore_id {
        None => Ok(blob_config),
        Some(inner_blobstore_id) => match blob_config {
            BlobConfig::MultiplexedWal { blobstores, .. } => {
                let seeked_id = BlobstoreId::new(inner_blobstore_id);
                blobstores
                    .into_iter()
                    .find_map(|(blobstore_id, _, blobstore)| {
                        if blobstore_id == seeked_id {
                            Some(remove_wrapper_blobconfigs(blobstore))
                        } else {
                            None
                        }
                    })
                    .ok_or_else(|| {
                        format_err!("could not find a blobstore with id {}", inner_blobstore_id)
                    })
            }
            _ => Err(format_err!(
                "inner-blobstore-id supplied but blobstore is not multiplexed"
            )),
        },
    }
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    let (_repo_name, repo_config) = app.repo_config(args.repo_args.as_repo_arg())?;

    let blob_config = get_blobconfig(
        repo_config.storage_config.blobstore,
        args.inner_blobstore_id,
    )?;
    let blobstore = app
        .open_blobstore_with_overridden_blob_config(&blob_config)
        .await?;

    match blobstore.unlink(&ctx, &args.key).await {
        Ok(_) => {
            writeln!(
                std::io::stdout(),
                "Unlinking key {} successfully in one underlying blobstore",
                args.key
            )?;
        }
        Err(e) => {
            writeln!(
                std::io::stdout(),
                "Failed to unlink key {} in one underlying blobstore, error: {}.",
                args.key,
                e
            )?;
        }
    }

    Ok(())
}
