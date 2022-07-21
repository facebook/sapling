/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
use std::sync::Arc;

use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use fbinit::FacebookInit;
use futures::future::try_join;

use blobstore::Blobstore;
use blobstore::BlobstoreGetData;
use blobstore_factory::make_blobstore;
use blobstore_factory::BlobstoreOptions;
use blobstore_factory::ReadOnlyStorage;
use cacheblob::new_memcache_blobstore;
use cacheblob::CacheBlobstoreExt;
use cached_config::ConfigStore;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use context::CoreContext;
use git_types::Tree as GitTree;
use mercurial_types::HgChangesetEnvelope;
use mercurial_types::HgFileEnvelope;
use mercurial_types::HgManifestEnvelope;
use metaconfig_types::BlobConfig;
use metaconfig_types::BlobstoreId;
use metaconfig_types::Redaction;
use metaconfig_types::StorageConfig;
use mononoke_types::FileContents;
use mononoke_types::RepositoryId;
use prefixblob::PrefixBlobstore;
use redactedblobstore::RedactedBlobs;
use redactedblobstore::RedactedBlobstore;
use redactedblobstore::RedactedBlobstoreConfig;
use redactedblobstore::RedactionConfigBlobstore;
use redactedblobstore::SqlRedactedContentStore;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::info;
use slog::warn;
use slog::Logger;
use sql_ext::facebook::MysqlOptions;
use std::ffi::OsStr;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tunables::tunables;

use crate::error::SubcommandError;

pub const BLOBSTORE_FETCH: &str = "blobstore-fetch";
const RAW_FILE_NAME_ARG: &str = "raw-blob";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(BLOBSTORE_FETCH)
        .about("fetches blobs from manifold")
        .args_from_usage("[KEY]    'key of the blob to be fetched with no repo prefex'")
        .arg(
            Arg::with_name("decode-as")
                .long("decode-as")
                .short("d")
                .takes_value(true)
                .possible_values(&["auto", "changeset", "manifest", "file", "contents", "git-tree"])
                .required(false)
                .help("if provided decode the value"),
        )
        .arg(
            Arg::with_name("use-memcache")
                .long("use-memcache")
                .short("m")
                .takes_value(true)
                .possible_values(&["cache-only", "no-fill", "fill-mc"])
                .required(false)
                .help("Use memcache to cache access to the blob store"),
        )
        .arg(
            Arg::with_name("no-prefix")
                .long("no-prefix")
                .short("P")
                .takes_value(false)
                .required(false)
                .help("Don't prepend a prefix based on the repo id to the key"),
        )
        .arg(
            Arg::with_name("inner-blobstore-id")
                .long("inner-blobstore-id")
                .takes_value(true)
                .required(false)
                .help("If main blobstore in the storage config is a multiplexed one, use inner blobstore with this id")
        )
        .arg(
            Arg::with_name(RAW_FILE_NAME_ARG)
                .long(RAW_FILE_NAME_ARG)
                .takes_value(true)
                .required(false)
                .help("Write the raw blob bytes to the given filename instead of printing to stdout."),
        )
}

fn get_blobconfig(blob_config: BlobConfig, inner_blobstore_id: Option<u64>) -> Result<BlobConfig> {
    match inner_blobstore_id {
        None => Ok(blob_config),
        Some(inner_blobstore_id) => match blob_config {
            BlobConfig::Multiplexed { blobstores, .. } => {
                let seeked_id = BlobstoreId::new(inner_blobstore_id);
                blobstores
                    .into_iter()
                    .find_map(|(blobstore_id, _, blobstore)| {
                        if blobstore_id == seeked_id {
                            Some(blobstore)
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

async fn get_blobstore<'a>(
    fb: FacebookInit,
    storage_config: StorageConfig,
    inner_blobstore_id: Option<u64>,
    mysql_options: &'a MysqlOptions,
    logger: Logger,
    readonly_storage: ReadOnlyStorage,
    blobstore_options: &'a BlobstoreOptions,
    config_store: &'a ConfigStore,
) -> Result<Arc<dyn Blobstore>, Error> {
    let blobconfig = get_blobconfig(storage_config.blobstore, inner_blobstore_id)?;

    make_blobstore(
        fb,
        blobconfig,
        mysql_options,
        readonly_storage,
        blobstore_options,
        &logger,
        config_store,
        &blobstore_factory::default_scrub_handler(),
        None,
    )
    .await
}

pub async fn subcommand_blobstore_fetch<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'a>,
    sub_m: &'a ArgMatches<'a>,
) -> Result<(), SubcommandError> {
    let config_store = matches.config_store();
    let repo_id = args::get_repo_id(config_store, matches)?;
    let (_, config) = args::get_config(config_store, matches)?;
    let redaction = config.redaction;
    let storage_config = config.storage_config;
    let inner_blobstore_id = args::get_u64_opt(&sub_m, "inner-blobstore-id");
    let mysql_options = matches.mysql_options();
    let blobstore_options = matches.blobstore_options();
    let readonly_storage = matches.readonly_storage();
    let blobstore_fut = get_blobstore(
        fb,
        storage_config,
        inner_blobstore_id,
        mysql_options,
        logger.clone(),
        *readonly_storage,
        blobstore_options,
        config_store,
    );

    let common_config = args::load_common_config(config_store, matches)?;
    let censored_scuba_params = common_config.censored_scuba_params;
    let mut scuba_redaction_builder =
        MononokeScubaSampleBuilder::with_opt_table(fb, censored_scuba_params.table);

    if let Some(scuba_log_file) = censored_scuba_params.local_path {
        scuba_redaction_builder = scuba_redaction_builder
            .with_log_file(scuba_log_file)
            .map_err(Error::from)?;
    }

    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let key = sub_m.value_of("KEY").unwrap().to_string();
    let decode_as = sub_m.value_of("decode-as").map(|val| val.to_string());
    let use_memcache = sub_m.value_of("use-memcache").map(|val| val.to_string());
    let no_prefix = sub_m.is_present("no-prefix");
    let maybe_output_file = sub_m.value_of_os(RAW_FILE_NAME_ARG);

    let maybe_redacted_blobs_fut = async {
        match redaction {
            Redaction::Enabled => {
                let redacted_blobs = if tunables().get_redaction_config_from_xdb() {
                    let redacted_blobs_db =
                        args::open_sql::<SqlRedactedContentStore>(fb, config_store, matches)?;
                    redacted_blobs_db.get_all_redacted_blobs().await?
                } else {
                    let redaction_config_blobstore = Arc::new(RedactionConfigBlobstore::new(
                        make_blobstore(
                            fb,
                            common_config.redaction_config.blobstore.clone(),
                            mysql_options,
                            *readonly_storage,
                            blobstore_options,
                            &logger,
                            config_store,
                            &blobstore_factory::default_scrub_handler(),
                            None,
                        )
                        .await?,
                    ));
                    RedactedBlobs::from_configerator(
                        config_store,
                        &common_config.redaction_config.redaction_sets_location,
                        ctx.clone(),
                        redaction_config_blobstore,
                    )
                    .await?
                };
                Ok(Some(Arc::new(redacted_blobs)))
            }
            Redaction::Disabled => Ok(None),
        }
    };

    let (blobstore, maybe_redacted_blobs) =
        try_join(blobstore_fut, maybe_redacted_blobs_fut).await?;

    info!(logger, "using blobstore: {:?}", blobstore);
    let value = get_from_sources(
        fb,
        use_memcache,
        blobstore,
        no_prefix,
        key.clone(),
        ctx,
        maybe_redacted_blobs,
        scuba_redaction_builder,
        repo_id,
    )
    .await?;

    output_blob(maybe_output_file, value.as_ref()).await?;
    if let Some(value) = value {
        let decode_as = decode_as.as_ref().and_then(|val| {
            let val = val.as_str();
            if val == "auto" {
                detect_decode(&key, &logger)
            } else {
                Some(val)
            }
        });

        match decode_as {
            Some("changeset") => display(&HgChangesetEnvelope::from_blob(value.into())),
            Some("manifest") => display(&HgManifestEnvelope::from_blob(value.into())),
            Some("file") => display(&HgFileEnvelope::from_blob(value.into())),
            // TODO: (rain1) T30974137 add a better way to print out file contents
            Some("contents") => println!(
                "{:?}",
                FileContents::from_encoded_bytes(value.into_raw_bytes())
            ),
            Some("git-tree") => display::<GitTree>(&value.try_into()),
            _ => {}
        }
    }

    Ok(())
}

async fn output_blob(path: Option<&OsStr>, value: Option<&BlobstoreGetData>) -> Result<(), Error> {
    match (path, value) {
        (None, _) | (_, None) => {
            println!("{:?}", value);
        }
        (Some(path), Some(value)) => {
            let mut file = File::create(path).await?;
            file.write_all(value.as_raw_bytes()).await?;
            file.flush().await?;
        }
    }
    Ok(())
}

async fn get_from_sources<T: Blobstore + Clone>(
    fb: FacebookInit,
    use_memcache: Option<String>,
    blobstore: T,
    no_prefix: bool,
    key: String,
    ctx: CoreContext,
    redacted_blobs: Option<Arc<RedactedBlobs>>,
    scuba_redaction_builder: MononokeScubaSampleBuilder,
    repo_id: RepositoryId,
) -> Result<Option<BlobstoreGetData>, Error> {
    let empty_prefix = "".to_string();

    match use_memcache {
        Some(mode) => {
            let blobstore = new_memcache_blobstore(fb, blobstore, "multiplexed", "").unwrap();
            let blobstore = match no_prefix {
                false => PrefixBlobstore::new(blobstore, repo_id.prefix()),
                true => PrefixBlobstore::new(blobstore, empty_prefix),
            };
            let blobstore = RedactedBlobstore::new(
                blobstore,
                RedactedBlobstoreConfig::new(redacted_blobs, scuba_redaction_builder),
            );
            get_cache(&ctx, &blobstore, &key, mode)
                .await
                .map(|opt_blob| opt_blob.map(Into::into))
        }
        None => {
            let blobstore = match no_prefix {
                false => PrefixBlobstore::new(blobstore, repo_id.prefix()),
                true => PrefixBlobstore::new(blobstore, empty_prefix),
            };
            let blobstore = RedactedBlobstore::new(
                blobstore,
                RedactedBlobstoreConfig::new(redacted_blobs, scuba_redaction_builder),
            );
            blobstore.get(&ctx, &key).await
        }
    }
}

fn display<T>(res: &Result<T>)
where
    T: fmt::Display + fmt::Debug,
{
    match res {
        Ok(val) => println!("---\n{}---", val),
        err => println!("{:?}", err),
    }
}

fn detect_decode(key: &str, logger: &Logger) -> Option<&'static str> {
    // Use a simple heuristic to figure out how to decode this key.
    if key.contains("hgchangeset.") {
        info!(logger, "Detected changeset key");
        Some("changeset")
    } else if key.contains("hgmanifest.") {
        info!(logger, "Detected manifest key");
        Some("manifest")
    } else if key.contains("hgfilenode.") {
        info!(logger, "Detected file key");
        Some("file")
    } else if key.contains("content.") {
        info!(logger, "Detected content key");
        Some("contents")
    } else if key.contains("git.tree.") {
        info!(logger, "Detected git-tree key");
        Some("git-tree")
    } else {
        warn!(
            logger,
            "Unable to detect how to decode this blob based on key";
            "key" => key,
        );
        None
    }
}

async fn get_cache<B: CacheBlobstoreExt>(
    ctx: &CoreContext,
    blobstore: &B,
    key: &str,
    mode: String,
) -> Result<Option<BlobstoreGetData>, Error> {
    if mode == "cache-only" {
        blobstore.get_cache_only(ctx, key).await
    } else if mode == "no-fill" {
        blobstore.get_no_cache_fill(ctx, key).await
    } else {
        blobstore.get(ctx, key).await
    }
}
