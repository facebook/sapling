// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use clap::ArgMatches;
use cmdlib::args;

use crate::cmdargs::{BLACKLIST_ADD, BLACKLIST_LIST, BLACKLIST_REMOVE};
use crate::common::{get_file_nodes, resolve_hg_rev};
use censoredblob::SqlCensoredContentStore;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{format_err, Error, FutureFailureErrorExt};
use futures::future::{self, join_all, Future};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use mercurial_types::{HgChangesetId, MPath};
use mononoke_types::{typed_hash::MononokeId, ContentId, Timestamp};
use slog::Logger;

use crate::error::SubcommandError;

/// Entrypoint for blacklist subcommand handling
pub fn subcommand_blacklist(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    match sub_m.subcommand() {
        (BLACKLIST_ADD, Some(sub_sub_m)) => blacklist_add(logger, matches, sub_sub_m),
        (BLACKLIST_REMOVE, Some(sub_sub_m)) => blacklist_remove(logger, matches, sub_sub_m),
        (BLACKLIST_LIST, Some(sub_sub_m)) => blacklist_list(logger, matches, sub_sub_m),
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
fn get_ctx_blobrepo_censored_blobs_cs_id(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> impl Future<
    Item = (
        CoreContext,
        BlobRepo,
        SqlCensoredContentStore,
        HgChangesetId,
    ),
    Error = SubcommandError,
> {
    let rev = match sub_m.value_of("hash") {
        Some(rev) => rev.to_string(),
        None => return future::err(SubcommandError::InvalidArgs).boxify(),
    };

    let ctx = CoreContext::test_mock();
    args::init_cachelib(&matches);

    let blobrepo = args::open_repo(&logger, &matches);
    let censored_blobs = args::open_sql::<SqlCensoredContentStore>(&matches)
        .context("While opening SqlCensoredContentStore")
        .from_err();

    blobrepo
        .and_then({
            cloned!(ctx);
            move |blobrepo| {
                resolve_hg_rev(ctx.clone(), &blobrepo, &rev).map(|cs_id| (blobrepo, cs_id))
            }
        })
        .join(censored_blobs)
        .map(move |((blobrepo, cs_id), censored_blobs)| (ctx, blobrepo, censored_blobs, cs_id))
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

fn blacklist_add(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    let (task, paths) = try_boxfuture!(task_and_paths_parser(sub_m));
    get_ctx_blobrepo_censored_blobs_cs_id(logger.clone(), matches, sub_m)
        .and_then(move |(ctx, blobrepo, censored_blobs, cs_id)| {
            content_ids_for_paths(ctx, logger, blobrepo, cs_id, paths)
                .and_then(move |content_ids| {
                    let blobstore_keys = content_ids
                        .iter()
                        .map(|content_id| content_id.blobstore_key())
                        .collect();
                    let timestamp = Timestamp::now();
                    censored_blobs.insert_censored_blobs(&blobstore_keys, &task, &timestamp)
                })
                .from_err()
        })
        .boxify()
}

fn blacklist_list(
    _logger: Logger,
    _matches: &ArgMatches<'_>,
    _sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    future::err(format_err!("listing not yet implemented").into()).boxify()
}

fn blacklist_remove(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    let paths = try_boxfuture!(paths_parser(sub_m));
    get_ctx_blobrepo_censored_blobs_cs_id(logger.clone(), matches, sub_m)
        .and_then(move |(ctx, blobrepo, censored_blobs, cs_id)| {
            content_ids_for_paths(ctx, logger, blobrepo, cs_id, paths)
                .and_then(move |content_ids| {
                    let blobstore_keys: Vec<_> = content_ids
                        .into_iter()
                        .map(|content_id| content_id.blobstore_key())
                        .collect();
                    censored_blobs.delete_censored_blobs(&blobstore_keys)
                })
                .from_err()
        })
        .boxify()
}
