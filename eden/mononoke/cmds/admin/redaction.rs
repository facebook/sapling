/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::common::get_file_nodes;
use anyhow::{anyhow, format_err, Error};
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use clap::{App, Arg, ArgGroup, ArgMatches, SubCommand};
use cloned::cloned;
use cmdlib::{args, helpers};
use context::CoreContext;
use failure_ext::FutureFailureErrorExt;
use fbinit::FacebookInit;
use futures::{compat::Future01CompatExt, future::TryFutureExt};
use futures_ext::{
    bounded_traversal::bounded_traversal_stream, try_boxfuture, BoxFuture, FutureExt,
};
use futures_old::{
    future::{self, join_all, Future},
    stream::Stream,
};
use itertools::{Either, Itertools};
use mercurial_types::{blobs::HgBlobChangeset, HgChangesetId, HgEntryId, HgManifest, MPath};
use mononoke_types::{typed_hash::MononokeId, ContentId, Timestamp};
use redactedblobstore::SqlRedactedContentStore;
use slog::{error, info, Logger};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::Arc;

use crate::error::SubcommandError;

pub const REDACTION: &str = "redaction";
const REDACTION_ADD: &str = "add";
const REDACTION_REMOVE: &str = "remove";
const REDACTION_LIST: &str = "list";
const ARG_FORCE: &str = "force";
const ARG_INPUT_FILE: &str = "input-file";
const ARG_MAIN_BOOKMARK: &str = "main-bookmark";
const DEFAULT_MAIN_BOOKMARK: &str = "master";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(REDACTION)
        .about("handle file redaction")
        .subcommand(add_path_parameters(
            SubCommand::with_name(REDACTION_ADD)
                .about("add a new redacted file at a given commit")
                .arg(
                    Arg::with_name("task")
                        .help("Task tracking the redaction request")
                        .takes_value(true)
                        .required(true),
                )
                .arg(
                    Arg::with_name("hash")
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
        ))
        .subcommand(add_path_parameters(
            SubCommand::with_name(REDACTION_REMOVE)
                .about("remove a file from the redaction")
                .arg(
                    Arg::with_name("hash")
                        .help("hg commit hash")
                        .takes_value(true)
                        .required(true),
                ),
        ))
        .subcommand(
            SubCommand::with_name(REDACTION_LIST)
                .about("list all redacted file for a given commit")
                .arg(
                    Arg::with_name("hash")
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

fn find_files_with_given_content_id_blobstore_keys(
    logger: Logger,
    ctx: CoreContext,
    repo: BlobRepo,
    cs: HgBlobChangeset,
    keys_to_tasks: HashMap<String, String>,
) -> impl Future<Item = Vec<(String, MPath, ContentId)>, Error = Error> {
    let manifest_id = cs.manifestid();
    let keys_to_tasks: Arc<HashMap<String, String>> = Arc::new(keys_to_tasks);
    bounded_traversal_stream(4096, Some((repo.clone(), manifest_id, None)), {
        cloned!(ctx);
        move |(repo, manifest_id, path)| {
            manifest_id
                .load(ctx.clone(), repo.blobstore())
                .compat()
                .from_err()
                .map({
                    cloned!(repo);
                    move |manifest| {
                        let (manifests, filenodes): (Vec<_>, Vec<_>) = HgManifest::list(&manifest)
                            .partition_map(|child| {
                                let full_path =
                                    MPath::join_element_opt(path.as_ref(), child.get_name());
                                match child.get_hash() {
                                    HgEntryId::File(_, filenode_id) => {
                                        Either::Right((full_path, filenode_id.clone()))
                                    }
                                    HgEntryId::Manifest(manifest_id) => {
                                        Either::Left((full_path, manifest_id.clone()))
                                    }
                                }
                            });

                        let children_manifests: Vec<_> = manifests
                            .into_iter()
                            .map(|(fp, mid)| (repo.clone(), mid, fp))
                            .collect();
                        (filenodes, children_manifests)
                    }
                })
        }
    })
    .map({
        cloned!(ctx, repo);
        move |filenodes| {
            let blobstore_key_futs = filenodes.into_iter().map({
                cloned!(ctx, repo);
                move |(full_path, filenode_id)| {
                    filenode_id
                        .load(ctx.clone(), repo.blobstore())
                        .compat()
                        .from_err()
                        .map(|env| (env.content_id(), full_path))
                }
            });
            join_all(blobstore_key_futs)
        }
    })
    .buffered(100)
    .fold((vec![], 0), {
        cloned!(logger, keys_to_tasks,);
        move |(mut collected_tasks_and_pairs, processed_files_count), keys_and_paths| {
            let mut pfc = processed_files_count;
            let filtered_tasks_and_pairs = keys_and_paths
                .into_iter()
                .filter_map({
                    |(key, full_path)| {
                        pfc += 1;
                        if pfc % 100_000 == 0 {
                            info!(logger.clone(), "Processed files: {}", pfc);
                        }
                        keys_to_tasks
                            .clone()
                            .get(&key.blobstore_key())
                            .map(|task| (task.clone(), key, full_path.clone()))
                    }
                })
                .map({
                    move |(task, key, full_path)| {
                        (task, full_path.expect("None MPath, yet not a root"), key)
                    }
                })
                .collect::<Vec<_>>();
            collected_tasks_and_pairs.extend(filtered_tasks_and_pairs);
            let res: Result<(Vec<_>, usize), Error> = Ok((collected_tasks_and_pairs, pfc));
            res
        }
    })
    .map(|(res, _)| res)
}

/// Entrypoint for redaction subcommand handling
pub async fn subcommand_redaction<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a ArgMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    match sub_m.subcommand() {
        (REDACTION_ADD, Some(sub_sub_m)) => redaction_add(fb, logger, matches, sub_sub_m).await,
        (REDACTION_REMOVE, Some(sub_sub_m)) => {
            redaction_remove(fb, logger, matches, sub_sub_m)
                .compat()
                .await
        }
        (REDACTION_LIST, Some(sub_sub_m)) => {
            redaction_list(fb, logger, matches, sub_sub_m)
                .compat()
                .await
        }
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

/// Fetch the task id and the file list from the subcommand cli matches
fn task_and_paths_parser(sub_m: &ArgMatches<'_>) -> Result<(String, Vec<MPath>), Error> {
    let task = match sub_m.value_of("task") {
        Some(task) => task.to_string(),
        None => return Err(format_err!("Task is needed")),
    };

    let paths = match paths_parser(sub_m) {
        Ok(paths) => paths,
        Err(e) => return Err(e),
    };
    Ok((task, paths))
}

/// Boilerplate to prepare a bunch of prerequisites for the rest of blaclisting operations
fn get_ctx_blobrepo_redacted_blobs_cs_id(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> impl Future<
    Item = (
        CoreContext,
        BlobRepo,
        SqlRedactedContentStore,
        HgChangesetId,
    ),
    Error = SubcommandError,
> {
    let rev = match sub_m.value_of("hash") {
        Some(rev) => rev.to_string(),
        None => return future::err(SubcommandError::InvalidArgs).boxify(),
    };

    args::init_cachelib(fb, &matches, None);

    let blobrepo = args::open_repo(fb, &logger, &matches);
    let redacted_blobs = args::open_sql::<SqlRedactedContentStore>(fb, &matches)
        .context("While opening SqlRedactedContentStore")
        .from_err();

    let ctx = CoreContext::new_with_logger(fb, logger);

    blobrepo
        .and_then({
            cloned!(ctx);
            move |blobrepo| {
                helpers::csid_resolve(ctx.clone(), blobrepo.clone(), rev.to_string())
                    .and_then({
                        cloned!(ctx, blobrepo);
                        move |cs_id| blobrepo.get_hg_from_bonsai_changeset(ctx, cs_id)
                    })
                    .map(|hg_cs_id| (blobrepo, hg_cs_id))
            }
        })
        .join(redacted_blobs)
        .map(move |((blobrepo, hg_cs_id), redacted_blobs)| {
            (ctx, blobrepo, redacted_blobs, hg_cs_id)
        })
        .from_err()
        .boxify()
}

/// Fetch a vector of `ContentId`s for a vector of `MPath`s
fn content_ids_for_paths(
    ctx: CoreContext,
    logger: Logger,
    blobrepo: BlobRepo,
    cs_id: HgChangesetId,
    paths: Vec<MPath>,
) -> impl Future<Item = Vec<ContentId>, Error = Error> {
    get_file_nodes(ctx.clone(), logger, &blobrepo, cs_id, paths)
        .and_then({
            move |hg_node_ids| {
                let content_ids = hg_node_ids.into_iter().map({
                    cloned!(blobrepo);
                    move |hg_node_id| {
                        hg_node_id
                            .load(ctx.clone(), blobrepo.blobstore())
                            .compat()
                            .from_err()
                            .map(|env| env.content_id())
                    }
                });

                join_all(content_ids)
            }
        })
        .from_err()
}

async fn redaction_add<'a, 'b>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a ArgMatches<'b>,
    sub_m: &'a ArgMatches<'b>,
) -> Result<(), SubcommandError> {
    let (task, paths) = task_and_paths_parser(sub_m)?;
    let (ctx, blobrepo, redacted_blobs, cs_id) =
        get_ctx_blobrepo_redacted_blobs_cs_id(fb, logger.clone(), matches, sub_m)
            .compat()
            .await?;

    let content_ids =
        content_ids_for_paths(ctx.clone(), logger.clone(), blobrepo.clone(), cs_id, paths)
            .compat()
            .await?;

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
            &blobstore_keys,
            main_bookmark,
            &task,
        )
        .await?;
    }

    let timestamp = Timestamp::now();
    redacted_blobs
        .insert_redacted_blobs(&blobstore_keys, &task, &timestamp)
        .compat()
        .await?;

    Ok(())
}

fn redaction_list(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    get_ctx_blobrepo_redacted_blobs_cs_id(fb, logger.clone(), matches, sub_m)
        .and_then(move |(ctx, blobrepo, redacted_blobs, cs_id)| {
            info!(
                logger,
                "Listing redacted files for ChangesetId: {:?}", cs_id
            );
            info!(logger, "Please be patient.");
            redacted_blobs
                .get_all_redacted_blobs()
                .join(
                    cs_id
                        .load(ctx.clone(), blobrepo.blobstore())
                        .compat()
                        .from_err(),
                )
                .and_then({
                    cloned!(logger);
                    move |(redacted_blobs, hg_cs)| {
                        find_files_with_given_content_id_blobstore_keys(
                            logger.clone(),
                            ctx,
                            blobrepo,
                            hg_cs,
                            redacted_blobs,
                        )
                        .map({
                            cloned!(logger);
                            move |mut res| {
                                if res.len() == 0 {
                                    info!(logger, "No files are redacted at this commit");
                                } else {
                                    res.sort();
                                    res.into_iter().for_each(|(task_id, file_path, _)| {
                                        info!(logger, "{:20}: {}", task_id, file_path);
                                    })
                                }
                            }
                        })
                    }
                })
                .from_err()
        })
        .boxify()
}

fn redaction_remove(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    let paths = try_boxfuture!(paths_parser(sub_m));
    get_ctx_blobrepo_redacted_blobs_cs_id(fb, logger.clone(), matches, sub_m)
        .and_then(move |(ctx, blobrepo, redacted_blobs, cs_id)| {
            content_ids_for_paths(ctx, logger, blobrepo, cs_id, paths)
                .and_then(move |content_ids| {
                    let blobstore_keys: Vec<_> = content_ids
                        .into_iter()
                        .map(|content_id| content_id.blobstore_key())
                        .collect();
                    redacted_blobs.delete_redacted_blobs(&blobstore_keys)
                })
                .from_err()
        })
        .boxify()
}

async fn check_if_content_is_reachable_from_bookmark(
    ctx: &CoreContext,
    blobrepo: &BlobRepo,
    keys_to_redact: &Vec<String>,
    main_bookmark: &str,
    task: &String,
) -> Result<(), Error> {
    let keys_to_tasks = keys_to_redact
        .clone()
        .into_iter()
        .map(|key| (key, task.clone()))
        .collect();

    info!(
        ctx.logger(),
        "Checking if redacted content exist in '{}' bookmark...", main_bookmark
    );
    let csid = helpers::csid_resolve(ctx.clone(), blobrepo.clone(), main_bookmark)
        .compat()
        .await?;
    let hg_cs_id = blobrepo
        .get_hg_from_bonsai_changeset(ctx.clone(), csid)
        .compat()
        .await?;

    let hg_cs = hg_cs_id
        .load(ctx.clone(), blobrepo.blobstore())
        .map_err(Error::from)
        .await?;

    let redacted_files = find_files_with_given_content_id_blobstore_keys(
        ctx.logger().clone(),
        ctx.clone(),
        blobrepo.clone(),
        hg_cs,
        keys_to_tasks,
    )
    .compat()
    .await?;
    let redacted_files_len = redacted_files.len();
    if redacted_files_len > 0 {
        for (_, path, content_id) in redacted_files {
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
        )
        .into());
    }

    Ok(())
}
