// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::{App, Arg, ArgMatches, SubCommand};
use cmdlib::args;

use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{format_err, Error};
use fbinit::FacebookInit;
use filenodes::FilenodeInfo;
use futures::future::{join_all, Future};
use futures::IntoFuture;
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::{HgFileNodeId, MPath};
use mononoke_types::RepoPath;
use slog::{debug, info, Logger};

use crate::common::{get_file_nodes, resolve_hg_rev};
use crate::error::SubcommandError;

const COMMAND_REVISION: &str = "by-revision";
const COMMAND_ID: &str = "by-id";

const ARG_REVISION: &str = "hg-changeset-or-bookmark";
const ARG_PATHS: &str = "paths";

const ARG_ID: &str = "id";
const ARG_PATH: &str = "path";

pub fn build_subcommand(name: &str) -> App {
    SubCommand::with_name(name)
        .about("fetches hg filenodes information for a commit and one or more paths")
        .subcommand(
            SubCommand::with_name(COMMAND_REVISION)
                .arg(
                    Arg::with_name(ARG_REVISION)
                        .required(true)
                        .takes_value(true)
                        .help("hg changeset to lookup filenodes for"),
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
}

fn extract_path(path: &str) -> Result<MPath, Error> {
    MPath::new(path).map_err(|err| format_err!("Could not parse path {}: {:?}", path, err))
}

fn log_filenode(logger: &Logger, filenode: &FilenodeInfo) {
    let FilenodeInfo {
        path,
        filenode,
        p1,
        p2,
        copyfrom,
        linknode,
    } = filenode;

    info!(
        logger,
        "Filenode {:?}:\n \
         -- path: {:?}\n \
         -- p1: {:?}\n \
         -- p2: {:?}\n \
         -- copyfrom: {:?}\n \
         -- linknode: {:?}",
        filenode,
        path,
        p1,
        p2,
        copyfrom,
        linknode
    );
}

fn handle_filenodes_at_revision(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    revision: &str,
    paths: Vec<MPath>,
) -> impl Future<Item = (), Error = Error> {
    resolve_hg_rev(ctx.clone(), &blobrepo, revision)
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
                            blobrepo.get_filenode(
                                ctx.clone(),
                                &RepoPath::FilePath(path),
                                filenode_id,
                            )
                        }),
                )
            }
        })
        .map({
            cloned!(ctx);
            move |filenodes| {
                filenodes
                    .into_iter()
                    .for_each(|filenode| log_filenode(ctx.logger(), &filenode))
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
                    handle_filenodes_at_revision(ctx, blobrepo, &rev, paths)
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
                    move |(blobrepo, path, id)| blobrepo.get_filenode(ctx.clone(), &path, id)
                })
                .map({
                    cloned!(ctx);
                    move |filenode| log_filenode(ctx.logger(), &filenode)
                })
                .from_err()
                .boxify()
        }
        _ => Err(SubcommandError::InvalidArgs).into_future().boxify(),
    }
}
