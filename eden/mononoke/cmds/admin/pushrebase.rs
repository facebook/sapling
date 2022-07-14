/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use clap_old::App;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use fbinit::FacebookInit;
use futures::TryFutureExt;

use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use context::CoreContext;
use maplit::hashset;
use metaconfig_types::BookmarkAttrs;
use pushrebase::do_pushrebase_bonsai;
use slog::Logger;

use crate::error::SubcommandError;

pub const ARG_BOOKMARK: &str = "bookmark";
pub const ARG_CSID: &str = "csid";
pub const PUSHREBASE: &str = "pushrebase";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(PUSHREBASE)
        .about("pushrebases a commit to a bookmark")
        .arg(
            Arg::with_name(ARG_CSID)
                .long(ARG_CSID)
                .takes_value(true)
                .required(true)
                .help("{hg|bonsai} changeset id or bookmark name"),
        )
        .arg(
            Arg::with_name(ARG_BOOKMARK)
                .long(ARG_BOOKMARK)
                .takes_value(true)
                .required(true)
                .help("name of the bookmark to pushrebase to"),
        )
}

pub async fn subcommand_pushrebase<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_matches: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo: BlobRepo = args::open_repo(fb, &logger, matches).await?;

    let cs_id = sub_matches
        .value_of(ARG_CSID)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_CSID))?;

    let cs_id = helpers::csid_resolve(&ctx, &repo, cs_id).await?;

    let bookmark = sub_matches
        .value_of(ARG_BOOKMARK)
        .ok_or_else(|| anyhow!("{} arg is not specified", ARG_BOOKMARK))?;

    let config_store = matches.config_store();
    let (_, repo_config) = args::get_config(config_store, matches)?;
    let bookmark = BookmarkName::new(bookmark)?;

    let pushrebase_flags = repo_config.pushrebase.flags;
    let bookmark_attrs = BookmarkAttrs::new(fb, repo_config.bookmarks.clone()).await?;
    let pushrebase_hooks = bookmarks_movement::get_pushrebase_hooks(
        &ctx,
        &repo,
        &bookmark,
        &bookmark_attrs,
        &repo_config.pushrebase,
    )
    .map_err(Error::from)?;

    let bcs = cs_id
        .load(&ctx, &repo.get_blobstore())
        .map_err(Error::from)
        .await?;
    let pushrebase_res = do_pushrebase_bonsai(
        &ctx,
        &repo,
        &pushrebase_flags,
        &bookmark,
        &hashset![bcs],
        &pushrebase_hooks,
    )
    .map_err(Error::from)
    .await?;

    println!("{}", pushrebase_res.head);

    Ok(())
}
