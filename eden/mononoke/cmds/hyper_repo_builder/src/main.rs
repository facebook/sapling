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
use cross_repo_sync::types::{Source, Target};
use fbinit::FacebookInit;
use futures::future::try_join;
use std::time::Duration;

use crate::tail::{find_source_repos, tail_once};

const ARG_HYPER_REPO_BOOKMARK_NAME: &str = "hyper-repo-bookmark-name";
const ARG_SOURCE_REPO: &str = "source-repo";
const ARG_SOURCE_REPO_BOOKMARK_NAME: &str = "source-repo-bookmark-name";
const SUBCOMMAND_ADD_SOURCE_REPO: &str = "add-source-repo";
const SUBCOMMAND_TAIL: &str = "tail";

mod add_source_repo;
mod common;
mod tail;

async fn subcommand_tail<'a>(
    fb: FacebookInit,
    matches: &'a MononokeMatches<'_>,
    _sub_m: &'a ArgMatches<'_>,
) -> Result<(), Error> {
    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let hyper_repo: BlobRepo = args::open_repo(ctx.fb, ctx.logger(), &matches).await?;

    let (source_bookmark, hyper_repo_bookmark) = parse_bookmarks(matches)?;

    let source_repos = find_source_repos(&ctx, &hyper_repo, &hyper_repo_bookmark, matches).await?;


    loop {
        tail_once(
            &ctx,
            source_repos.clone(),
            hyper_repo.clone(),
            &source_bookmark,
            &hyper_repo_bookmark,
        )
        .await?;

        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

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

    let (source_bookmark, hyper_repo_bookmark) = parse_bookmarks(matches)?;

    add_source_repo::add_source_repo(
        &ctx,
        &source_repo,
        &hyper_repo,
        &source_bookmark,
        &hyper_repo_bookmark,
    )
    .await?;

    Ok(())
}

async fn run<'a>(fb: FacebookInit, matches: &'a MononokeMatches<'_>) -> Result<(), Error> {
    match matches.subcommand() {
        (SUBCOMMAND_ADD_SOURCE_REPO, Some(sub_m)) => {
            subcommand_add_source_repo(fb, &matches, &sub_m).await
        }
        (SUBCOMMAND_TAIL, Some(sub_m)) => subcommand_tail(fb, &matches, &sub_m).await,
        (subcommand, _) => Err(anyhow!("unknown subcommand {}!", subcommand)),
    }
}

fn parse_bookmarks(
    matches: &MononokeMatches<'_>,
) -> Result<(Source<BookmarkName>, Target<BookmarkName>), Error> {
    let hyper_repo_bookmark = matches
        .value_of(ARG_HYPER_REPO_BOOKMARK_NAME)
        .ok_or_else(|| anyhow!("{} is not set", ARG_HYPER_REPO_BOOKMARK_NAME))?;

    let source_bookmark = matches
        .value_of(ARG_SOURCE_REPO_BOOKMARK_NAME)
        .ok_or_else(|| anyhow!("{} is not set", ARG_SOURCE_REPO_BOOKMARK_NAME))?;
    Ok((
        Source(BookmarkName::new(source_bookmark)?),
        Target(BookmarkName::new(hyper_repo_bookmark)?),
    ))
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
        .arg(
            Arg::with_name(ARG_HYPER_REPO_BOOKMARK_NAME)
                .long(ARG_HYPER_REPO_BOOKMARK_NAME)
                .required(true)
                .takes_value(true)
                .help("Name of the bookmark in hyper repo to sync to"),
        )
        .arg(
            Arg::with_name(ARG_SOURCE_REPO_BOOKMARK_NAME)
                .long(ARG_SOURCE_REPO_BOOKMARK_NAME)
                .required(true)
                .takes_value(true)
                .help("Name of the bookmark in source repos to sync from"),
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
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_TAIL).about("Tail source repos into hyper repo"),
        )
        .get_matches(fb)?;

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run(fb, &matches))
}
