// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use clap::ArgMatches;
use cmdlib::args;

use crate::common::{get_file_nodes, resolve_hg_rev};
use cloned::cloned;
use context::CoreContext;
use failure_ext::{format_err, Error};
use filenodes::FilenodeInfo;
use futures::future::{join_all, Future};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use mercurial_types::MPath;
use mononoke_types::RepoPath;
use slog::{debug, info, Logger};

use crate::error::SubcommandError;

pub fn subcommand_filenodes(
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_m: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    let rev = sub_m
        .value_of("hg-changeset-or-bookmark")
        .unwrap()
        .to_string();

    let paths: Result<Vec<_>, Error> = sub_m
        .values_of("paths")
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

    args::open_repo(&logger, &matches)
        .and_then({
            cloned!(ctx);
            move |blobrepo| {
                resolve_hg_rev(ctx.clone(), &blobrepo, &rev).map(|cs_id| (blobrepo, cs_id))
            }
        })
        .and_then({
            cloned!(ctx, logger);
            move |(blobrepo, cs_id)| {
                debug!(logger, "using commit: {:?}", cs_id);
                get_file_nodes(ctx.clone(), logger.clone(), &blobrepo, cs_id, paths.clone())
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
        .map(move |filenodes| {
            filenodes.into_iter().for_each(|filenode| {
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
            });
        })
        .from_err()
        .boxify()
}
