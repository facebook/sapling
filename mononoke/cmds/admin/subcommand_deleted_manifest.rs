/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::error::SubcommandError;
use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use clap::{App, Arg, ArgMatches, SubCommand};
use cloned::cloned;
use cmdlib::{args, helpers};
use context::CoreContext;
use deleted_files_manifest::{
    iterate_entries, RootDeletedManifestId, RootDeletedManifestMapping, Status,
};
use derived_data::BonsaiDerived;
use fbinit::FacebookInit;
use futures::{future::err, stream::futures_unordered, Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, FutureExt};
use manifest::get_implicit_deletes;
use mercurial_types::HgManifestId;
use mononoke_types::{ChangesetId, MPath};
use revset::AncestorsNodeStream;
use slog::{debug, Logger};
use std::{collections::BTreeSet, sync::Arc};

const COMMAND_MANIFEST: &'static str = "manifest";
const COMMAND_VERIFY: &'static str = "verify";
const ARG_CSID: &'static str = "csid";
const ARG_LIMIT: &'static str = "limit";
const ARG_PATH: &'static str = "path";

pub fn subcommand_deleted_manifest_build(name: &str) -> App {
    let csid_arg = Arg::with_name(ARG_CSID)
        .help("{hg|boinsai} changset id or bookmark name")
        .index(1)
        .required(true);

    let path_arg = Arg::with_name(ARG_PATH)
        .help("path")
        .index(2)
        .default_value("");

    SubCommand::with_name(name)
        .about("derive, inspect and verify deleted files manifest")
        .subcommand(
            SubCommand::with_name(COMMAND_MANIFEST)
                .help("recursively list all deleted files manifest entries under the given path")
                .arg(csid_arg.clone())
                .arg(path_arg.clone()),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_VERIFY)
                .help("verify deleted manifest against actual paths deleted in commits")
                .arg(csid_arg.clone())
                .arg(
                    Arg::with_name(ARG_LIMIT)
                        .help("number of commits to be verified")
                        .takes_value(true)
                        .required(true),
                ),
        )
}

pub fn subcommand_deleted_manifest(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_matches: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    args::init_cachelib(fb, &matches);

    let repo = args::open_repo(fb, &logger, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    match sub_matches.subcommand() {
        (COMMAND_MANIFEST, Some(matches)) => {
            let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
            let path = match matches.value_of(ARG_PATH).unwrap() {
                "" => Ok(None),
                p => MPath::new(p).map(Some),
            };

            (repo, path)
                .into_future()
                .and_then(move |(repo, path)| {
                    helpers::csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
                        .and_then(move |cs_id| subcommand_manifest(ctx, repo, cs_id, path))
                })
                .from_err()
                .boxify()
        }
        (COMMAND_VERIFY, Some(matches)) => {
            let hash_or_bookmark = String::from(matches.value_of(ARG_CSID).unwrap());
            let limit = matches
                .value_of(ARG_LIMIT)
                .unwrap()
                .parse::<u64>()
                .expect("limit must be an integer");

            cloned!(ctx);
            repo.into_future()
                .and_then(move |repo| {
                    helpers::csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
                        .and_then(move |cs_id| subcommand_verify(ctx, repo, cs_id, limit))
                })
                .from_err()
                .boxify()
        }
        _ => err(SubcommandError::InvalidArgs).boxify(),
    }
}

fn subcommand_manifest(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
    prefix: Option<MPath>,
) -> impl Future<Item = (), Error = Error> {
    let mapping = Arc::new(RootDeletedManifestMapping::new(repo.get_blobstore()));
    RootDeletedManifestId::derive(ctx.clone(), repo.clone(), mapping, cs_id)
        .and_then(move |root_manifest| {
            debug!(
                ctx.logger(),
                "ROOT Deleted Files Manifest {:?}", root_manifest,
            );

            let mf_id = root_manifest.deleted_manifest_id().clone();
            iterate_entries(ctx.clone(), repo.clone(), mf_id).collect()
        })
        .map(move |path_states: Vec<_>| {
            let mut paths = path_states
                .into_iter()
                .filter_map(move |(path, st, mf_id)| match st {
                    Status::Deleted(_) => {
                        if let Some(pref) = &prefix {
                            let elems = MPath::iter_opt(path.as_ref());
                            if pref.is_prefix_of(elems) {
                                Some((path, mf_id))
                            } else {
                                None
                            }
                        } else {
                            Some((path, mf_id))
                        }
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();

            paths.sort_by_key(|(path, _)| path.clone());

            for (path, mf_id) in paths {
                println!("{}/ {:?}", MPath::display_opt(path.as_ref()), mf_id);
            }
        })
}

fn subcommand_verify(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
    limit: u64,
) -> impl Future<Item = (), Error = Error> {
    AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), cs_id)
        .take(limit)
        .for_each(move |cs_id| verify_single_commit(ctx.clone(), repo.clone(), cs_id))
}

fn get_parents(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
) -> impl Future<Item = Vec<HgManifestId>, Error = Error> {
    cloned!(ctx, repo);
    repo.get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
        .and_then({
            cloned!(ctx, repo);
            move |hg_cs_id| repo.get_changeset_parents(ctx.clone(), hg_cs_id)
        })
        .and_then({
            move |parent_hg_cs_ids| {
                cloned!(ctx, repo);
                let parents = parent_hg_cs_ids.into_iter().map(|cs_id| {
                    repo.get_changeset_by_changesetid(ctx.clone(), cs_id)
                        .map(move |blob_changeset| blob_changeset.manifestid().clone())
                });

                futures_unordered(parents).collect()
            }
        })
}

fn get_file_changes(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
) -> impl Future<Item = (Vec<MPath>, Vec<MPath>), Error = Error> {
    let paths_added_fut = cs_id
        .load(ctx.clone(), &repo.get_blobstore())
        .from_err()
        .map(move |bonsai| {
            bonsai
                .into_mut()
                .file_changes
                .into_iter()
                .filter_map(|(path, change)| {
                    if let Some(_) = change {
                        Some(path)
                    } else {
                        None
                    }
                })
                .collect()
        });

    paths_added_fut
        .join(get_parents(ctx.clone(), repo.clone(), cs_id))
        .and_then(
            move |(paths_added, parent_manifests): (Vec<MPath>, Vec<HgManifestId>)| {
                get_implicit_deletes(
                    ctx.clone(),
                    repo.get_blobstore(),
                    paths_added.clone(),
                    parent_manifests,
                )
                .collect()
                .map(move |paths_deleted| (paths_added, paths_deleted))
            },
        )
}

fn verify_single_commit(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
) -> impl Future<Item = (), Error = Error> {
    let file_changes = get_file_changes(ctx.clone(), repo.clone(), cs_id.clone());

    let mapping = Arc::new(RootDeletedManifestMapping::new(repo.get_blobstore()));
    let deleted_manifest_paths =
        RootDeletedManifestId::derive(ctx.clone(), repo.clone(), mapping, cs_id)
            .and_then({
                cloned!(ctx, repo);
                move |root_manifest| {
                    let mf_id = root_manifest.deleted_manifest_id().clone();
                    iterate_entries(ctx.clone(), repo.clone(), mf_id).collect()
                }
            })
            .map(move |entries: Vec<_>| {
                entries
                    .into_iter()
                    .filter_map(move |(path_opt, st, ..)| match (path_opt, st) {
                        (Some(path), Status::Deleted(_)) => Some(path),
                        _ => None,
                    })
                    .collect::<BTreeSet<_>>()
            });

    file_changes.join(deleted_manifest_paths).and_then(
        move |((paths_added, paths_deleted), deleted_manifest_paths)| {
            for path in paths_added {
                // check that changed files are alive
                if deleted_manifest_paths.contains(&path) {
                    println!("Path {} is alive in changeset {:?}", path, cs_id);
                    return Err(format_err!("Path {} is alive", path));
                }
            }
            for path in paths_deleted {
                // check that deleted files are in the manifest
                if !deleted_manifest_paths.contains(&path) {
                    println!("Path {} was deleted in changeset {:?}", path, cs_id);
                    return Err(format_err!("Path {} is deleted", path));
                }
            }

            Ok(())
        },
    )
}
