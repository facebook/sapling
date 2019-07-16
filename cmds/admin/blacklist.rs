// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use clap::ArgMatches;
use cmdlib::args;

use crate::common::resolve_hg_rev;
use censoredblob::SqlCensoredContentStore;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{format_err, Error, FutureFailureErrorExt};
use futures::future;
use futures::future::{join_all, Future};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use mercurial_types::{Changeset, HgFileNodeId, MPath};
use mononoke_types::{typed_hash::MononokeId, ContentId, Timestamp};
use slog::{debug, Logger};

pub fn subcommand_blacklist(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), Error> {
    let rev = sub_m.value_of("hash").unwrap().to_string();
    let task = sub_m.value_of("task").unwrap().to_string();

    let paths: Result<Vec<_>, Error> = sub_m
        .values_of("FILES_LIST")
        .expect("at least one file")
        .map(|path| {
            let mpath = MPath::new(path);
            match mpath {
                Ok(mpath) => Ok(mpath),
                Err(_) => Err(format_err!(
                    "The following path could not be parsed {}",
                    path
                )),
            }
        })
        .collect();

    let paths: Vec<_> = try_boxfuture!(paths);

    let ctx = CoreContext::test_mock();
    args::init_cachelib(&matches);

    let censored_blobs = args::open_sql::<SqlCensoredContentStore>(&matches)
        .context("While opening SqlCensoredContentStore")
        .from_err();

    let blobrepo = args::open_repo(&logger, &matches);

    blobrepo
        .join(censored_blobs)
        .and_then({
            move |(blobrepo, censored_blobs)| {
                get_file_nodes(ctx.clone(), logger.clone(), &blobrepo, &rev, paths).and_then({
                    move |hg_node_ids| {
                        let content_ids = hg_node_ids.into_iter().map(move |hg_node_id| {
                            blobrepo.get_file_content_id(ctx.clone(), hg_node_id)
                        });

                        debug!(logger, "Inserting all the blobstore keys in the database");
                        join_all(content_ids).and_then(move |content_ids: Vec<ContentId>| {
                            let blobstore_keys = content_ids
                                .iter()
                                .map(|content_id| content_id.blobstore_key())
                                .collect();
                            let timestamp = Timestamp::now();
                            censored_blobs.insert_censored_blobs(&blobstore_keys, &task, &timestamp)
                        })
                    }
                })
            }
        })
        .boxify()
}

// The function retrieves the ContentId of a file, based on path and rev.
// If the path is not valid an error is expected.
fn get_file_nodes(
    ctx: CoreContext,
    logger: Logger,
    repo: &BlobRepo,
    rev: &str,
    paths: Vec<MPath>,
) -> impl Future<Item = Vec<HgFileNodeId>, Error = Error> {
    let resolved_cs_id = resolve_hg_rev(ctx.clone(), repo, rev);

    resolved_cs_id
        .and_then({
            cloned!(ctx, repo);
            move |cs_id| repo.get_changeset_by_changesetid(ctx, cs_id)
        })
        .map(|cs| cs.manifestid().clone())
        .and_then({
            cloned!(ctx, repo);
            move |root_mf_id| {
                repo.find_files_in_manifest(ctx, root_mf_id, paths.clone())
                    .map(move |manifest_entries| {
                        let mut existing_hg_nodes = Vec::new();
                        let mut non_existing_paths = Vec::new();

                        for path in paths.iter() {
                            match manifest_entries.get(&path) {
                                Some(hg_node) => existing_hg_nodes.push(*hg_node),
                                None => non_existing_paths.push(path.clone()),
                            };
                        }
                        (non_existing_paths, existing_hg_nodes)
                    })
            }
        })
        .and_then({
            move |(non_existing_paths, existing_hg_nodes)| match non_existing_paths.len() {
                0 => {
                    debug!(logger, "All the file paths are valid");
                    future::ok(existing_hg_nodes).right_future()
                }
                _ => future::err(format_err!(
                    "failed to identify the files associated with the file paths {:?}",
                    non_existing_paths
                ))
                .left_future(),
            }
        })
}
