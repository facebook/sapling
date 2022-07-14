/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::common::get_file_nodes;
use anyhow::anyhow;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use blobstore::Storable;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgGroup;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use cloned::cloned;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future::try_join_all;
use futures::future::TryFutureExt;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use manifest::ManifestOps;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_types::blobs::HgBlobChangeset;
use mercurial_types::HgChangesetId;
use mercurial_types::MPath;
use mononoke_types::blob::BlobstoreValue;
use mononoke_types::typed_hash::BlobstoreKey;
use mononoke_types::ContentId;
use mononoke_types::RedactionKeyList;
use mononoke_types::Timestamp;
use redactedblobstore::SqlRedactedContentStore;
use repo_factory::RepoFactory;
use slog::error;
use slog::info;
use slog::Logger;
use std::collections::HashSet;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;

use crate::error::SubcommandError;

pub const REDACTION: &str = "redaction";
const REDACTION_ADD: &str = "add";
const REDACTION_CREATE_KEY_LIST: &str = "create-key-list";
const REDACTION_CREATE_KEY_LIST_RAW: &str = "create-key-list-from-ids";
const REDACTION_REMOVE: &str = "remove";
const REDACTION_LIST: &str = "list";
const ARG_KEYS: &str = "keys";
const ARG_HASH: &str = "hash";
const ARG_TASK: &str = "task";
const ARG_LOG_ONLY: &str = "log-only";
const ARG_FORCE: &str = "force";
const ARG_INPUT_FILE: &str = "input-file";
const ARG_MAIN_BOOKMARK: &str = "main-bookmark";
const DEFAULT_MAIN_BOOKMARK: &str = "master";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(REDACTION)
        .about("handle file redaction")
        .subcommand(add_path_parameters(
            SubCommand::with_name(REDACTION_CREATE_KEY_LIST)
                .about("Add a new redaction key list to the blobstore, to be later configured via configerator.")
                .arg(
                    Arg::with_name(ARG_HASH)
                        .help("hg id, bonsai id or bookmark")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name(ARG_MAIN_BOOKMARK)
                        .long(ARG_MAIN_BOOKMARK)
                        .takes_value(true)
                        .required(false)
                        .default_value(DEFAULT_MAIN_BOOKMARK)
                        .help("Redaction fails if any of the content to be redacted is present in --main-bookmark unless --force is set.")
                )
                .arg(
                    Arg::with_name(ARG_FORCE)
                        .long(ARG_FORCE)
                        .takes_value(false)
                        .help("by default redaction fails if any of the redacted files is in main-bookmark. This flag overrides it.")
                )
        ))
        .subcommand(
            SubCommand::with_name(REDACTION_CREATE_KEY_LIST_RAW)
                .about("Add a new redaction set to the blobstore, to be later configured via configerator. This is given hashes directly.")
                .arg(
                    Arg::with_name(ARG_KEYS)
                        .long(ARG_KEYS)
                        .multiple(true)
                        .takes_value(true)
                        .help("List of keys")
                )
        )
        .subcommand(add_path_parameters(
            SubCommand::with_name(REDACTION_ADD)
                .about("add a new redacted file at a given commit")
                .arg(
                    Arg::with_name(ARG_TASK)
                        .help("Task tracking the redaction request")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name(ARG_HASH)
                        .help("hg commit hash")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name(ARG_MAIN_BOOKMARK)
                        .long(ARG_MAIN_BOOKMARK)
                        .takes_value(true)
                        .required(false)
                        .default_value(DEFAULT_MAIN_BOOKMARK)
                        .help("Redaction fails if any of the content to be redacted is present in --main-bookmark unless --force is set.")
                )
                .arg(
                    Arg::with_name(ARG_FORCE)
                        .long(ARG_FORCE)
                        .takes_value(false)
                        .help("by default redaction fails if any of the redacted files is in main-bookmark. This flag overrides it.")
                )
                .arg(
                    Arg::with_name(ARG_LOG_ONLY)
                        .long(ARG_LOG_ONLY)
                        .takes_value(false)
                        .help("redact file in log-only mode. All accesses to this file will be allowed, but they will all be logged")
                )
        ))
        .subcommand(add_path_parameters(
            SubCommand::with_name(REDACTION_REMOVE)
                .about("remove a file from the redaction")
                .arg(
                    Arg::with_name(ARG_HASH)
                        .help("hg commit hash")
                        .takes_value(true)
                        .required(true),
                ),
        ))
        .subcommand(
            SubCommand::with_name(REDACTION_LIST)
                .about("list all redacted file for a given commit")
                .arg(
                    Arg::with_name(ARG_HASH)
                        .help("hg commit hash or a bookmark")
                        .takes_value(true)
                        .required(true),
                ),
        )
}

pub fn add_path_parameters<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(ARG_INPUT_FILE)
            .long(ARG_INPUT_FILE)
            .help("file with a list of filenames to redact")
            .takes_value(true)
            .required(false),
    )
    .args_from_usage(
        r#"
                [FILES_LIST]...                             'list of files to be be redacted'
                "#,
    )
    .group(
        ArgGroup::with_name("input_files")
            .args(&["FILES_LIST", ARG_INPUT_FILE])
            .required(true),
    )
}

async fn find_files_with_given_content_id_blobstore_keys<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    cs: HgBlobChangeset,
    keys: HashSet<&String>,
) -> Result<Vec<(MPath, ContentId)>, Error> {
    let manifest_id = cs.manifestid();
    let mut s = manifest_id
        .list_leaf_entries(ctx.clone(), repo.get_blobstore())
        .map_ok(|(full_path, (_, filenode_id))| async move {
            let env = filenode_id.load(ctx, repo.blobstore()).await?;
            Result::<_, Error>::Ok((env.content_id(), full_path))
        })
        .try_buffer_unordered(100);

    let mut paths_and_content_ids = vec![];
    let mut processed_files_count = 0usize;
    while let Some(key_and_path) = s.next().await {
        let (key, full_path) = key_and_path?;
        processed_files_count += 1;
        if processed_files_count % 100_000 == 0 {
            info!(ctx.logger(), "Processed files: {}", processed_files_count);
        }

        if keys.contains(&key.blobstore_key()) {
            paths_and_content_ids.push((full_path, key));
        }
    }
    Ok(paths_and_content_ids)
}

/// Entrypoint for redaction subcommand handling
pub async fn subcommand_redaction<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    match sub_m.subcommand() {
        (REDACTION_CREATE_KEY_LIST, Some(sub_sub_m)) => {
            redaction_create_key_list(fb, logger, matches, sub_sub_m).await
        }
        (REDACTION_CREATE_KEY_LIST_RAW, Some(sub_sub_m)) => {
            redaction_create_key_list_raw(fb, logger, matches, sub_sub_m).await
        }
        (REDACTION_ADD, Some(sub_sub_m)) => redaction_add(fb, logger, matches, sub_sub_m).await,
        (REDACTION_REMOVE, Some(sub_sub_m)) => {
            redaction_remove(fb, logger, matches, sub_sub_m).await
        }
        (REDACTION_LIST, Some(sub_sub_m)) => redaction_list(fb, logger, matches, sub_sub_m).await,
        _ => {
            eprintln!("{}", matches.usage());
            ::std::process::exit(1);
        }
    }
}

/// Fetch the file list from the subcommand cli matches
fn paths_parser(sub_m: &ArgMatches<'_>) -> Result<Vec<MPath>, Error> {
    match sub_m.values_of("FILES_LIST") {
        Some(values) => values.map(|s| s.to_string()).map(MPath::new).collect(),
        None => match sub_m.value_of(ARG_INPUT_FILE) {
            Some(inputfile) => {
                let inputfile = File::open(inputfile)?;
                let input_file = BufReader::new(&inputfile);
                let mut files = vec![];
                for line in input_file.lines() {
                    let line = line?;
                    files.push(MPath::new(line)?);
                }

                Ok(files)
            }
            None => {
                return Err(format_err!("file list is not specified"));
            }
        },
    }
}

fn task_parser(sub_m: &ArgMatches<'_>) -> Result<String, Error> {
    match sub_m.value_of(ARG_TASK) {
        Some(task) => Ok(task.to_string()),
        None => Err(format_err!("Task is needed")),
    }
}

/// Fetch the task id and the file list from the subcommand cli matches
fn task_and_paths_parser(sub_m: &ArgMatches<'_>) -> Result<(String, Vec<MPath>), Error> {
    Ok((task_parser(sub_m)?, paths_parser(sub_m)?))
}

/// Boilerplate to prepare a bunch of prerequisites for the rest of blaclisting operations
async fn get_ctx_blobrepo_cs_id<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(CoreContext, BlobRepo, HgChangesetId), SubcommandError> {
    let rev = match sub_m.value_of(ARG_HASH) {
        Some(rev) => rev.to_string(),
        None => return Err(SubcommandError::InvalidArgs),
    };

    let blobrepo: BlobRepo = args::open_repo(fb, &logger, matches).await?;

    let ctx = CoreContext::new_with_logger(fb, logger);

    let cs_id = helpers::csid_resolve(&ctx, blobrepo.clone(), rev.to_string()).await?;
    let hg_cs_id = blobrepo.derive_hg_changeset(&ctx, cs_id).await?;

    Ok((ctx, blobrepo, hg_cs_id))
}

/// Fetch a vector of `ContentId`s for a vector of `MPath`s
async fn content_ids_for_paths(
    ctx: CoreContext,
    logger: Logger,
    blobrepo: BlobRepo,
    cs_id: HgChangesetId,
    paths: Vec<MPath>,
) -> Result<Vec<ContentId>, Error> {
    let hg_node_ids = get_file_nodes(ctx.clone(), logger, &blobrepo, cs_id, paths).await?;
    let content_ids = hg_node_ids.into_iter().map(|hg_node_id| {
        cloned!(ctx, blobrepo);
        async move {
            let env = hg_node_id.load(&ctx, blobrepo.blobstore()).await?;
            Ok(env.content_id())
        }
    });
    try_join_all(content_ids).await
}

async fn redaction_create_key_list_impl<'a, 'b>(
    _fb: FacebookInit,
    _logger: Logger,
    matches: &'a MononokeMatches<'b>,
    ctx: &'a CoreContext,
    blobstore_keys: Vec<String>,
) -> Result<(), SubcommandError> {
    let common = args::load_common_config(matches.config_store(), matches)?;
    let factory = RepoFactory::new(matches.environment().clone(), &common);

    let blobstore = factory.redaction_config_blobstore().await?;
    let darkstorm_blobstore = factory
        .redaction_config_blobstore_from_config(
            &common.redaction_config.darkstorm_blobstore.ok_or_else(|| {
                anyhow!("Admin tier config must have darkstorm_blobstore field set")
            })?,
        )
        .await?;

    let store_keys = |blobstore| {
        cloned!(blobstore_keys);
        async move {
            RedactionKeyList {
                keys: blobstore_keys,
            }
            .into_blob()
            .store(ctx, &blobstore)
            .await
        }
    };

    let (id, id2) = futures::try_join!(store_keys(blobstore), store_keys(darkstorm_blobstore))?;

    if id != id2 {
        Err(format_err!(
            "Id mismatch on darkstorm and non-darkstorm blobstores: {} vs {}",
            id,
            id2
        ))?
    }

    println!("Redaction saved as: {}", id);
    println!(
        "To finish the redaction process, you need to commit this id to \
        scm/mononoke/redaction/redaction_sets.cconf on configerator"
    );
    Ok(())
}

async fn redaction_create_key_list<'a, 'b>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'b>,
    sub_m: &'a ArgMatches<'b>,
) -> Result<(), SubcommandError> {
    let paths = paths_parser(sub_m)?;
    let (ctx, blobrepo, cs_id) = get_ctx_blobrepo_cs_id(fb, logger.clone(), matches, sub_m).await?;
    let content_ids =
        content_ids_for_paths(ctx.clone(), logger.clone(), blobrepo.clone(), cs_id, paths).await?;
    let blobstore_keys: Vec<_> = content_ids
        .iter()
        .map(|content_id| content_id.blobstore_key())
        .collect();

    let force = sub_m.is_present(ARG_FORCE);
    if !force {
        let main_bookmark = sub_m
            .value_of(ARG_MAIN_BOOKMARK)
            .unwrap_or(DEFAULT_MAIN_BOOKMARK);
        check_if_content_is_reachable_from_bookmark(
            &ctx,
            &blobrepo,
            blobstore_keys.iter().collect(),
            main_bookmark,
        )
        .await?;
    }

    redaction_create_key_list_impl(fb, logger, matches, &ctx, blobstore_keys).await
}

async fn redaction_create_key_list_raw<'a, 'b>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'b>,
    sub_m: &'a ArgMatches<'b>,
) -> Result<(), SubcommandError> {
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let keys = sub_m
        .values_of(ARG_KEYS)
        .ok_or(SubcommandError::InvalidArgs)?
        .map(|key| key.to_string())
        .collect::<Vec<_>>();

    redaction_create_key_list_impl(fb, logger, matches, &ctx, keys).await
}

async fn redaction_add<'a, 'b>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'b>,
    sub_m: &'a ArgMatches<'b>,
) -> Result<(), SubcommandError> {
    let (task, paths) = task_and_paths_parser(sub_m)?;
    let (ctx, blobrepo, cs_id) = get_ctx_blobrepo_cs_id(fb, logger.clone(), matches, sub_m).await?;
    let redacted_blobs =
        args::open_sql::<SqlRedactedContentStore>(fb, matches.config_store(), matches)
            .context("While opening SqlRedactedContentStore")?;

    let content_ids =
        content_ids_for_paths(ctx.clone(), logger.clone(), blobrepo.clone(), cs_id, paths).await?;

    let blobstore_keys: Vec<_> = content_ids
        .iter()
        .map(|content_id| content_id.blobstore_key())
        .collect();

    let force = sub_m.is_present(ARG_FORCE);
    let log_only = sub_m.is_present(ARG_LOG_ONLY);

    if !force {
        let main_bookmark = sub_m
            .value_of(ARG_MAIN_BOOKMARK)
            .unwrap_or(DEFAULT_MAIN_BOOKMARK);
        check_if_content_is_reachable_from_bookmark(
            &ctx,
            &blobrepo,
            blobstore_keys.iter().collect(),
            main_bookmark,
        )
        .await?;
    }

    let timestamp = Timestamp::now();
    redacted_blobs
        .insert_redacted_blobs(&blobstore_keys, &task, &timestamp, log_only)
        .await?;

    Ok(())
}

async fn redaction_list<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let (ctx, blobrepo, cs_id) = get_ctx_blobrepo_cs_id(fb, logger.clone(), matches, sub_m).await?;
    let redacted_blobs =
        args::open_sql::<SqlRedactedContentStore>(fb, matches.config_store(), matches)
            .context("While opening SqlRedactedContentStore")?;
    info!(
        logger,
        "Listing redacted files for ChangesetId: {:?}", cs_id
    );
    info!(logger, "Please be patient.");
    let (redacted_blobs, hg_cs) = futures::try_join!(
        redacted_blobs.get_all_redacted_blobs(),
        cs_id.load(&ctx, blobrepo.blobstore()).map_err(Error::from),
    )?;
    let redacted_map = redacted_blobs.redacted();
    let redacted_keys = redacted_map.iter().map(|(key, _)| key).collect();
    let path_keys =
        find_files_with_given_content_id_blobstore_keys(&ctx, &blobrepo, hg_cs, redacted_keys)
            .await?;
    let mut res = path_keys
        .into_iter()
        .filter_map(move |(path, key)| {
            redacted_map
                .get(&key.blobstore_key())
                .cloned()
                .map(|redacted_meta| (redacted_meta.task, path, redacted_meta.log_only))
        })
        .collect::<Vec<_>>();
    if res.is_empty() {
        info!(logger, "No files are redacted at this commit");
    } else {
        res.sort();
        res.into_iter().for_each(|(task_id, file_path, log_only)| {
            let log_only_msg = if log_only { " (log only)" } else { "" };
            info!(logger, "{:20}: {}{}", task_id, file_path, log_only_msg);
        })
    }
    Ok(())
}

async fn redaction_remove<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let paths = paths_parser(sub_m)?;
    let (ctx, blobrepo, cs_id) = get_ctx_blobrepo_cs_id(fb, logger.clone(), matches, sub_m).await?;
    let redacted_blobs =
        args::open_sql::<SqlRedactedContentStore>(fb, matches.config_store(), matches)
            .context("While opening SqlRedactedContentStore")?;
    let content_ids = content_ids_for_paths(ctx, logger, blobrepo, cs_id, paths).await?;
    let blobstore_keys: Vec<_> = content_ids
        .into_iter()
        .map(|content_id| content_id.blobstore_key())
        .collect();
    redacted_blobs
        .delete_redacted_blobs(&blobstore_keys)
        .await
        .map_err(SubcommandError::Error)
}

async fn check_if_content_is_reachable_from_bookmark(
    ctx: &CoreContext,
    blobrepo: &BlobRepo,
    keys_to_redact: HashSet<&String>,
    main_bookmark: &str,
) -> Result<(), Error> {
    info!(
        ctx.logger(),
        "Checking if redacted content exist in '{}' bookmark...", main_bookmark
    );
    let csid = helpers::csid_resolve(ctx, blobrepo, main_bookmark).await?;
    let hg_cs_id = blobrepo.derive_hg_changeset(ctx, csid).await?;

    let hg_cs = hg_cs_id.load(ctx, blobrepo.blobstore()).await?;

    let redacted_files =
        find_files_with_given_content_id_blobstore_keys(ctx, blobrepo, hg_cs, keys_to_redact)
            .await?;
    let redacted_files_len = redacted_files.len();
    if redacted_files_len > 0 {
        for (path, content_id) in redacted_files {
            error!(
                ctx.logger(),
                "Redacted in {}: {} {}",
                main_bookmark,
                path,
                content_id.blobstore_key()
            );
        }
        return Err(anyhow!(
            "{} files will be redacted in {}. \
            That means that checking it out will be impossible!",
            redacted_files_len,
            main_bookmark,
        ));
    }

    Ok(())
}
