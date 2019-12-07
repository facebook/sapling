/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use clap::{App, Arg, ArgMatches, SubCommand};
use cmdlib::args;

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use cloned::cloned;
use cmdlib::helpers;
use context::CoreContext;
use fbinit::FacebookInit;
use filenodes::FilenodeInfo;
use futures::future::{join_all, Future};
use futures::{IntoFuture, Stream};
use futures_ext::{BoxFuture, FutureExt};
use manifest::{Entry, ManifestOps};
use mercurial_types::{HgFileEnvelope, HgFileNodeId, MPath};
use mononoke_types::RepoPath;
use slog::{debug, info, Logger};

use crate::common::get_file_nodes;
use crate::error::SubcommandError;

const COMMAND_ID: &str = "by-id";
const COMMAND_REVISION: &str = "by-revision";
const COMMAND_VALIDATE: &str = "validate";

const ARG_ENVELOPE: &str = "envelope";

const ARG_REVISION: &str = "changeset-id";
const ARG_PATHS: &str = "paths";

const ARG_ID: &str = "id";
const ARG_PATH: &str = "path";

pub fn build_subcommand(name: &str) -> App {
    SubCommand::with_name(name)
        .about("fetches hg filenodes information for a commit and one or more paths")
        .arg(
            Arg::with_name(ARG_ENVELOPE)
                .long(ARG_ENVELOPE)
                .required(false)
                .takes_value(false)
                .help("whether to show the envelope as well"),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_REVISION)
                .arg(
                    Arg::with_name(ARG_REVISION)
                        .required(true)
                        .takes_value(true)
                        .help("hg/bonsai changeset id or bookmark to lookup filenodes for"),
                )
                .arg(
                    Arg::with_name(ARG_PATHS)
                        .required(true)
                        .multiple(true)
                        .takes_value(true)
                        .help("a list of file paths to lookup filenodes for"),
                ),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_ID)
                .arg(
                    Arg::with_name(ARG_PATH)
                        .required(true)
                        .takes_value(true)
                        .help("path to lookup filenode for (use trailing / for directories)"),
                )
                .arg(
                    Arg::with_name(ARG_ID)
                        .required(true)
                        .takes_value(true)
                        .help("filenode ID"),
                ),
        )
        .subcommand(
            SubCommand::with_name(COMMAND_VALIDATE)
                .about("for a public changeset validates that all files and trees exist")
                .arg(
                    Arg::with_name(ARG_REVISION)
                        .required(true)
                        .takes_value(true)
                        .help("hg/bonsai changeset id or bookmark"),
                ),
        )
}

fn extract_path(path: &str) -> Result<MPath, Error> {
    MPath::new(path).map_err(|err| format_err!("Could not parse path {}: {:?}", path, err))
}

fn log_filenode(
    logger: &Logger,
    filenode: &FilenodeInfo,
    envelope: Option<&HgFileEnvelope>,
) -> Option<()> {
    let FilenodeInfo {
        path,
        filenode,
        p1,
        p2,
        copyfrom,
        linknode,
    } = filenode;

    info!(logger, "Filenode: {:?}", filenode);
    info!(logger, "-- path: {:?}", path);
    info!(logger, "-- p1: {:?}", p1);
    info!(logger, "-- p2: {:?}", p2);
    info!(logger, "-- copyfrom: {:?}", copyfrom);
    info!(logger, "-- linknode: {:?}", linknode);
    info!(logger, "-- content id: {:?}", envelope?.content_id());
    info!(logger, "-- content size: {:?}", envelope?.content_size());

    Some(())
}

fn handle_filenodes_at_revision(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    revision: &str,
    paths: Vec<MPath>,
    log_envelope: bool,
) -> impl Future<Item = (), Error = Error> {
    helpers::csid_resolve(ctx.clone(), blobrepo.clone(), revision.to_string())
        .and_then({
            cloned!(ctx, blobrepo);
            move |cs_id| blobrepo.get_hg_from_bonsai_changeset(ctx, cs_id)
        })
        .map(|cs_id| (blobrepo, cs_id))
        .and_then({
            cloned!(ctx);
            move |(blobrepo, cs_id)| {
                debug!(ctx.logger(), "using commit: {:?}", cs_id);
                get_file_nodes(
                    ctx.clone(),
                    ctx.logger().clone(),
                    &blobrepo,
                    cs_id,
                    paths.clone(),
                )
                .map(|filenode_ids| (blobrepo, paths.into_iter().zip(filenode_ids.into_iter())))
            }
        })
        .and_then({
            cloned!(ctx);
            move |(blobrepo, path_filenode_ids)| {
                join_all(
                    path_filenode_ids
                        .into_iter()
                        .map(move |(path, filenode_id)| {
                            let filenode = blobrepo.get_filenode(
                                ctx.clone(),
                                &RepoPath::FilePath(path),
                                filenode_id,
                            );

                            let envelope = if log_envelope {
                                blobrepo
                                    .get_file_envelope(ctx.clone(), filenode_id)
                                    .map(Some)
                                    .left_future()
                            } else {
                                Ok(None).into_future().right_future()
                            };

                            (filenode, envelope)
                        }),
                )
            }
        })
        .map({
            cloned!(ctx);
            move |filenodes| {
                filenodes.into_iter().for_each(|(filenode, envelope)| {
                    log_filenode(ctx.logger(), &filenode, envelope.as_ref());
                })
            }
        })
}

pub fn subcommand_filenodes(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    args::init_cachelib(fb, &matches);

    let blobrepo = args::open_repo(fb, &ctx.logger(), &matches);
    let log_envelope = sub_m.is_present(ARG_ENVELOPE);

    match sub_m.subcommand() {
        (COMMAND_REVISION, Some(matches)) => {
            let rev = matches.value_of(ARG_REVISION).unwrap().to_string();

            let paths: Result<Vec<_>, Error> = matches
                .values_of(ARG_PATHS)
                .unwrap()
                .map(extract_path)
                .collect();

            (blobrepo, Ok(rev).into_future(), paths)
                .into_future()
                .and_then(move |(blobrepo, rev, paths)| {
                    handle_filenodes_at_revision(ctx, blobrepo, &rev, paths, log_envelope)
                })
                .from_err()
                .boxify()
        }
        (COMMAND_ID, Some(matches)) => {
            let path = matches.value_of(ARG_PATH).unwrap();
            let path = match path.chars().last() {
                Some('/') => extract_path(&path).map(RepoPath::DirectoryPath),
                Some(_) => extract_path(&path).map(RepoPath::FilePath),
                None => Ok(RepoPath::RootPath),
            };

            let id = matches.value_of(ARG_ID).unwrap().parse::<HgFileNodeId>();

            (blobrepo, path.into_future(), id.into_future())
                .into_future()
                .and_then({
                    cloned!(ctx);
                    move |(blobrepo, path, id)| {
                        blobrepo
                            .get_filenode(ctx.clone(), &path, id)
                            .and_then(move |filenode| {
                                let envelope = if log_envelope {
                                    blobrepo
                                        .get_file_envelope(ctx, filenode.filenode)
                                        .map(Some)
                                        .left_future()
                                } else {
                                    Ok(None).into_future().right_future()
                                };

                                (Ok(filenode), envelope)
                            })
                    }
                })
                .map({
                    cloned!(ctx);
                    move |(filenode, envelope)| {
                        log_filenode(ctx.logger(), &filenode, envelope.as_ref());
                    }
                })
                .from_err()
                .boxify()
        }
        (COMMAND_VALIDATE, Some(matches)) => {
            let rev = matches.value_of(ARG_REVISION).unwrap().to_string();
            let ctx = CoreContext::new_with_logger(fb, logger.clone());

            blobrepo
                .and_then(move |repo| {
                    helpers::get_root_manifest_id(ctx.clone(), repo.clone(), rev).and_then(
                        move |mf_id| {
                            mf_id
                                .list_all_entries(ctx.clone(), repo.get_blobstore())
                                .map(move |(path, entry)| {
                                    let (repo_path, filenode_id) = match entry {
                                        Entry::Leaf((_, filenode_id)) => (
                                            RepoPath::FilePath(
                                                path.expect("unexpected empty file path"),
                                            ),
                                            filenode_id,
                                        ),
                                        Entry::Tree(mf_id) => {
                                            let filenode_id =
                                                HgFileNodeId::new(mf_id.into_nodehash());
                                            match path {
                                                Some(path) => {
                                                    (RepoPath::DirectoryPath(path), filenode_id)
                                                }
                                                None => (RepoPath::RootPath, filenode_id),
                                            }
                                        }
                                    };

                                    repo.get_filenode_opt(ctx.clone(), &repo_path, filenode_id)
                                        .and_then(move |maybe_filenode| {
                                            if maybe_filenode.is_some() {
                                                Ok(())
                                            } else {
                                                Err(format_err!(
                                                    "not found filenode for {}",
                                                    repo_path
                                                ))
                                            }
                                        })
                                })
                                .buffer_unordered(100)
                                .for_each(|_| Ok(()))
                        },
                    )
                })
                .map(|_| ())
                .from_err()
                .boxify()
        }
        _ => Err(SubcommandError::InvalidArgs).into_future().boxify(),
    }
}
