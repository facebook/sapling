/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use blobrepo::BlobRepo;
use clap::ArgMatches;
use cmdlib::{args, helpers};

use crate::cmdargs::{REDACTION_ADD, REDACTION_LIST, REDACTION_REMOVE};
use crate::common::get_file_nodes;
use anyhow::{format_err, Error};
use cloned::cloned;
use context::CoreContext;
use failure_ext::FutureFailureErrorExt;
use fbinit::FacebookInit;
use futures::future::{self, join_all, Future};
use futures::stream::Stream;
use futures_ext::{
    bounded_traversal::bounded_traversal_stream, try_boxfuture, BoxFuture, FutureExt,
};
use itertools::{Either, Itertools};
use mercurial_types::{blobs::HgBlobChangeset, HgChangesetId, HgEntryId, MPath};
use mononoke_types::{typed_hash::MononokeId, ContentId, Timestamp};
use redactedblobstore::SqlRedactedContentStore;
use slog::{info, Logger};
use std::collections::HashMap;
use std::sync::Arc;

use crate::error::SubcommandError;

fn find_files_with_given_content_id_blobstore_keys(
    logger: Logger,
    ctx: CoreContext,
    repo: BlobRepo,
    cs: HgBlobChangeset,
    keys_to_tasks: HashMap<String, String>,
) -> impl Future<Item = Vec<(String, String)>, Error = Error> {
    let manifest_id = cs.manifestid();
    let keys_to_tasks: Arc<HashMap<String, String>> = Arc::new(keys_to_tasks);
    bounded_traversal_stream(4096, Some((repo.clone(), manifest_id, None)), {
        cloned!(ctx);
        move |(repo, manifest_id, path)| {
            repo.clone()
                .get_manifest_by_nodeid(ctx.clone(), manifest_id)
                .map({
                    cloned!(repo);
                    move |manifest| {
                        let (manifests, filenodes): (Vec<_>, Vec<_>) =
                            manifest.list().partition_map(|child| {
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
                    repo.get_file_content_id(ctx.clone(), filenode_id)
                        .map(|content_id| (content_id.blobstore_key(), full_path))
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
                            .get(&key)
                            .map(|task| (task.clone(), full_path.clone()))
                    }
                })
                .map({
                    move |(task, full_path)| {
                        let full_path =
                            format!("{}", full_path.expect("None MPath, yet not a root"));
                        (task, full_path)
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
pub fn subcommand_redaction(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    match sub_m.subcommand() {
        (REDACTION_ADD, Some(sub_sub_m)) => redaction_add(fb, logger, matches, sub_sub_m),
        (REDACTION_REMOVE, Some(sub_sub_m)) => redaction_remove(fb, logger, matches, sub_sub_m),
        (REDACTION_LIST, Some(sub_sub_m)) => redaction_list(fb, logger, matches, sub_sub_m),
        _ => {
            eprintln!("{}", matches.usage());
            ::std::process::exit(1);
        }
    }
}

/// Fetch the file list from the subcommand cli matches
fn paths_parser(sub_m: &ArgMatches<'_>) -> Result<Vec<MPath>, Error> {
    let paths: Result<Vec<_>, Error> = match sub_m.values_of("FILES_LIST") {
        Some(values) => values,
        None => return Err(format_err!("File list is needed")),
    }
    .map(|path| MPath::new(path))
    .collect();
    paths
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

    args::init_cachelib(fb, &matches);

    let blobrepo = args::open_repo(fb, &logger, &matches);
    let redacted_blobs = args::open_sql::<SqlRedactedContentStore>(&matches)
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
                    move |hg_node_id| blobrepo.get_file_content_id(ctx.clone(), hg_node_id)
                });

                join_all(content_ids)
            }
        })
        .from_err()
}

fn redaction_add(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    let (task, paths) = try_boxfuture!(task_and_paths_parser(sub_m));
    get_ctx_blobrepo_redacted_blobs_cs_id(fb, logger.clone(), matches, sub_m)
        .and_then(move |(ctx, blobrepo, redacted_blobs, cs_id)| {
            content_ids_for_paths(ctx, logger, blobrepo, cs_id, paths)
                .and_then(move |content_ids| {
                    let blobstore_keys = content_ids
                        .iter()
                        .map(|content_id| content_id.blobstore_key())
                        .collect();
                    let timestamp = Timestamp::now();
                    redacted_blobs.insert_redacted_blobs(&blobstore_keys, &task, &timestamp)
                })
                .from_err()
        })
        .boxify()
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
                "Listing blacklisted files for ChangesetId: {:?}", cs_id
            );
            info!(logger, "Please be patient.");
            let changeset_fut = blobrepo.get_changeset_by_changesetid(ctx.clone(), cs_id);
            redacted_blobs
                .get_all_redacted_blobs()
                .join(changeset_fut)
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
                                    info!(logger, "No files are blacklisted at this commit");
                                } else {
                                    res.sort();
                                    res.into_iter().for_each(|(task_id, file_path)| {
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
