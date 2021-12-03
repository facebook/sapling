/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This tool can be used to build a large test repo (hyper repo) from a few other repos.
//! This tool mirrors all commits from a given small repo bookmark into a hyper repo.
//! All files from small repos are put in a large repo in a folder with the same name
//! as small repo e.g. file "1.txt" from "small_repo_1" will become "small_repo_1/1.txt" file
//! in hyper repo.

#![deny(warnings)]

use anyhow::{anyhow, Error};
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use clap::{Arg, ArgMatches, SubCommand};
use cmdlib::args::{self, MononokeMatches};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future::try_join;

const BOOKMARK_NAME: &str = "master";
const ARG_SOURCE_REPO: &str = "source-repo";
const SUBCOMMAND_ADD_SOURCE_REPO: &str = "add-source-repo";

mod add_source_repo;
mod common;

async fn subcommand_add_source_repo<'a>(
    fb: FacebookInit,
    matches: &'a MononokeMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), Error> {
    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let source_repo_name = sub_m
        .value_of(ARG_SOURCE_REPO)
        .ok_or_else(|| anyhow!("source repo is not set"))?;
    let source_repo = args::open_repo_with_repo_name(
        ctx.fb,
        &ctx.logger(),
        source_repo_name.to_string(),
        matches,
    );

    let hyper_repo = args::open_repo(ctx.fb, ctx.logger(), &matches);

    let (source_repo, hyper_repo): (BlobRepo, BlobRepo) = try_join(source_repo, hyper_repo).await?;

    let bookmark_name = BookmarkName::new(BOOKMARK_NAME)?;

    add_source_repo::add_source_repo(&ctx, &source_repo, &hyper_repo, &bookmark_name).await?;

    Ok(())
}

async fn run<'a>(fb: FacebookInit, matches: &'a MononokeMatches<'_>) -> Result<(), Error> {
    match matches.subcommand() {
        (SUBCOMMAND_ADD_SOURCE_REPO, Some(sub_m)) => {
            subcommand_add_source_repo(fb, &matches, &sub_m).await
        }
        (subcommand, _) => Err(anyhow!("unknown subcommand {}!", subcommand)),
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let matches = args::MononokeAppBuilder::new("Hyper repo builder")
        .with_advanced_args_hidden()
        .build()
        .about(
            "A tool to create a merged repo out of a few other repos. \
        It can be useful for testing the scalability limits e.g. limits on commit rate.",
        )
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_ADD_SOURCE_REPO)
                .about("Add new source repo to existing hyper repo")
                .arg(
                    Arg::with_name(ARG_SOURCE_REPO)
                        .long(ARG_SOURCE_REPO)
                        .required(true)
                        .takes_value(true)
                        .help("new repo to add to a hyper repo"),
                ),
        )
        .get_matches(fb)?;

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run(fb, &matches))
}
