/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::io::BufRead;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use blobstore_factory::make_packblob;
use borrowed::borrowed;
use clap::Parser;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::stream;
use futures::stream::TryStreamExt;
use metaconfig_types::BlobConfig;
use metaconfig_types::BlobstoreId;
use mononoke_app::args::RepoArgs;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;

mod pack_utils;

#[derive(Parser)]
#[clap(
    about = "Given a set of blob names on stdin, replace them with a packed version that takes less space"
)]
struct MononokePackerArgs {
    #[clap(flatten)]
    repo_args: RepoArgs,

    #[clap(
        long,
        help = "If main blobstore in the storage config is a multiplexed one, use inner blobstore with this id"
    )]
    inner_blobstore_id: Option<u64>,

    #[clap(long, help = "zstd compression level to use")]
    zstd_level: i32,

    #[clap(
        long,
        help = "If true, do not upload the finished pack to the blobstore"
    )]
    dry_run: bool,

    #[clap(
        long,
        default_value_t = 10,
        help = "Maximum number of parallel packs to work on. Default 10"
    )]
    scheduled_max: usize,
}

const PACK_PREFIX: &str = "multiblob-";

fn get_blobconfig(
    mut blob_config: BlobConfig,
    inner_blobstore_id: Option<u64>,
) -> Result<BlobConfig> {
    // If the outer store is a mux, find th requested inner store
    if let Some(inner_blobstore_id) = inner_blobstore_id {
        blob_config = match blob_config {
            BlobConfig::Multiplexed { blobstores, .. }
            | BlobConfig::MultiplexedWal { blobstores, .. } => {
                let required_id = BlobstoreId::new(inner_blobstore_id);
                blobstores
                    .into_iter()
                    .find_map(|(blobstore_id, _, blobstore)| {
                        if blobstore_id == required_id {
                            Some(blobstore)
                        } else {
                            None
                        }
                    })
                    .with_context(|| {
                        format!("could not find a blobstore with id {}", inner_blobstore_id)
                    })?
            }
            _ => bail!("inner-blobstore-id can only be supplied for multiplexed blobstores"),
        }
    };

    Ok(blob_config)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app: MononokeApp = MononokeAppBuilder::new(fb)
        .with_app_extension(Fb303AppExtension {})
        .build::<MononokePackerArgs>()?;

    let args: MononokePackerArgs = app.args()?;
    let inner_id = args.inner_blobstore_id;
    let zstd_level = args.zstd_level;
    let dry_run = args.dry_run;
    let max_parallelism = args.scheduled_max;

    let env = app.environment();
    let logger = app.logger();
    let runtime = app.runtime();
    let config_store = app.config_store();

    let ctx = CoreContext::new_for_bulk_processing(fb, logger.clone());
    let readonly_storage = &env.readonly_storage;
    let blobstore_options = &env.blobstore_options;

    let repo_arg = args.repo_args.id_or_name();
    let (_repo_name, repo_config) = app.repo_config(repo_arg)?;
    let blobconfig = repo_config.storage_config.blobstore;
    let repo_prefix = repo_config.repoid.prefix();

    let input_lines: Vec<String> = io::stdin()
        .lock()
        .lines()
        .collect::<Result<_, io::Error>>()?;

    let mut scuba = env.scuba_sample_builder.clone();
    scuba.add_opt("blobstore_id", inner_id);

    runtime.block_on(async move {
        let blobstore = make_packblob(
            fb,
            get_blobconfig(blobconfig, inner_id)?,
            *readonly_storage,
            blobstore_options,
            logger,
            config_store,
        )
        .await?;
        stream::iter(input_lines.split(String::is_empty).map(Result::Ok))
            .try_for_each_concurrent(max_parallelism, |pack_keys| {
                borrowed!(ctx, repo_prefix, blobstore, scuba);
                async move {
                    let pack_keys: Vec<&str> = pack_keys.iter().map(|i| i.as_ref()).collect();
                    pack_utils::repack_keys(
                        ctx,
                        blobstore,
                        PACK_PREFIX,
                        zstd_level,
                        repo_prefix,
                        &pack_keys,
                        dry_run,
                        scuba,
                    )
                    .await
                }
            })
            .await
    })
}
