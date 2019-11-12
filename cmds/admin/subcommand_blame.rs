/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::error::SubcommandError;
use blame::fetch_blame;
use blobrepo::BlobRepo;
use clap::{App, Arg, ArgMatches, SubCommand};
use cmdlib::{args, helpers};
use context::CoreContext;
use failure::Error;
use fbinit::FacebookInit;
use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use mononoke_types::{ChangesetId, MPath};
use slog::Logger;

const ARG_CSID: &'static str = "csid";
const ARG_PATH: &'static str = "path";

pub fn subcommand_blame_build(name: &str) -> App {
    SubCommand::with_name(name)
        .about("fetch/derive blame for specified changeset and path")
        .arg(
            Arg::with_name(ARG_CSID)
                .help("{hg|bonsai} changeset id or bookmark name")
                .index(1)
                .required(true),
        )
        .arg(
            Arg::with_name(ARG_PATH)
                .help("path")
                .index(2)
                .required(true),
        )
}

pub fn subcommand_blame(
    fb: FacebookInit,
    logger: Logger,
    matches: &ArgMatches<'_>,
    sub_matches: &ArgMatches<'_>,
) -> BoxFuture<(), SubcommandError> {
    args::init_cachelib(fb, &matches);

    let repo = args::open_repo(fb, &logger, &matches);
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let hash_or_bookmark = String::from(sub_matches.value_of(ARG_CSID).unwrap());
    let path = MPath::new(sub_matches.value_of(ARG_PATH).unwrap());

    (repo, path)
        .into_future()
        .and_then({
            move |(repo, path)| {
                helpers::csid_resolve(ctx.clone(), repo.clone(), hash_or_bookmark)
                    .and_then(move |csid| subcommand_show_blame(ctx, repo, csid, path))
            }
        })
        .from_err()
        .boxify()
}

fn subcommand_show_blame(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: MPath,
) -> impl Future<Item = (), Error = Error> {
    fetch_blame(ctx, repo, csid, path)
        .from_err()
        .and_then(|(content, blame)| {
            let content_str = String::from_utf8_lossy(content.as_ref());
            println!("{}", blame.annotate(content_str.as_ref())?);
            Ok(())
        })
}
