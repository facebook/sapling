/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! This tool can be used to build a large test repo (hyper repo) from a few other repos.
//! This tool mirrors all commits from a given small repo bookmark into a hyper repo.
//! This tool can be used to check e.g. speed of derived data derivation.
//! All files from small repos are put in a large repo in a folder with the same name
//! as small repo e.g. file "1.txt" from "small_repo_1" will become "small_repo_1/1.txt" file
//! in hyper repo.
//! To start using it use "add-source-repo" to add a new repo to a hyper repo. Add as many repos
//! as you like.
//! Then use "tail" to tail new commits from source repos to hyper repos
//!
//! LIMITATIONS:
//! 1) Non-forward bookmark moves are not supported
//! 2) Syncing merges is not supported

use anyhow::anyhow;
use anyhow::Error;
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use clap_old::Arg;
use clap_old::ArgMatches;
use clap_old::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use context::CoreContext;
use cross_repo_sync::types::Source;
use cross_repo_sync::types::Target;
use cross_repo_sync::validation::verify_working_copy_inner;
use fbinit::FacebookInit;
use futures::future::try_join;
use slog::info;
use std::time::Duration;

use crate::common::find_source_repos;
use crate::common::find_source_repos_and_latest_synced_cs_ids;
use crate::common::get_mover_and_reverse_mover;
use crate::tail::tail_once;

const ARG_CHANGESET: &str = "changeset";
const ARG_HYPER_REPO_BOOKMARK_NAME: &str = "hyper-repo-bookmark-name";
const ARG_PER_COMMIT_FILE_CHANGES_LIMIT: &str = "per-commit-file-changes-limit";
const ARG_SOURCE_REPO: &str = "source-repo";
const ARG_SOURCE_REPO_BOOKMARK_NAME: &str = "source-repo-bookmark-name";
const ARG_ONCE: &str = "once";
const DEFAULT_FILE_CHANGES_LIMIT: usize = 10000;
const SUBCOMMAND_ADD_SOURCE_REPO: &str = "add-source-repo";
const SUBCOMMAND_TAIL: &str = "tail";
const SUBCOMMAND_VALIDATE: &str = "validate";

mod add_source_repo;
mod common;
mod tail;

async fn subcommand_tail<'a>(
    fb: FacebookInit,
    matches: &'a MononokeMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), Error> {
    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let hyper_repo: BlobRepo = args::open_repo(ctx.fb, ctx.logger(), matches).await?;

    let (source_bookmark, hyper_repo_bookmark) = parse_bookmarks(matches)?;

    let hyper_repo_tip_cs_id = hyper_repo
        .get_bonsai_bookmark(ctx.clone(), &hyper_repo_bookmark)
        .await?
        .ok_or_else(|| anyhow!("{} bookmark not found in hyper repo", hyper_repo_bookmark))?;

    let source_repos = find_source_repos(&ctx, &hyper_repo, hyper_repo_tip_cs_id, matches).await?;

    // Let's be extra cautious. hyper repo is expected to be a test repo, so no backup
    // config should be present. If it's present then something might be wrong, and we
    // might be accidentally syncing data to a prod repo.
    let config_store = matches.config_store();
    let (_, config) = args::get_config(config_store, matches)?;
    if config.backup_repo_config.is_some() {
        return Err(anyhow!(
            "hyper repo unexpectedly has a backup repo. \
        Since hyper repo is expected to be a test repo it's not expected that it has a backup. \
        As a precaution we fail instead of writing to a non-test repo."
        ));
    }

    loop {
        tail_once(
            &ctx,
            source_repos.clone(),
            hyper_repo.clone(),
            &source_bookmark,
            &hyper_repo_bookmark,
        )
        .await?;

        if sub_m.is_present(ARG_ONCE) {
            break Ok(());
        }
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
    let source_repo =
        args::open_repo_with_repo_name(ctx.fb, ctx.logger(), source_repo_name.to_string(), matches);

    let hyper_repo = args::open_repo(ctx.fb, ctx.logger(), matches);

    let (source_repo, hyper_repo): (BlobRepo, BlobRepo) = try_join(source_repo, hyper_repo).await?;

    let (source_bookmark, hyper_repo_bookmark) = parse_bookmarks(matches)?;

    add_source_repo::add_source_repo(
        &ctx,
        &source_repo,
        &hyper_repo,
        &source_bookmark,
        &hyper_repo_bookmark,
        Some(args::get_usize(
            sub_m,
            ARG_PER_COMMIT_FILE_CHANGES_LIMIT,
            DEFAULT_FILE_CHANGES_LIMIT,
        )),
    )
    .await?;

    Ok(())
}

async fn subcommand_validate<'a>(
    fb: FacebookInit,
    matches: &'a MononokeMatches<'_>,
    sub_m: &'a ArgMatches<'_>,
) -> Result<(), Error> {
    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    let hyper_repo: BlobRepo = args::open_repo(ctx.fb, ctx.logger(), matches).await?;

    let hyper_cs_id = sub_m
        .value_of(ARG_CHANGESET)
        .ok_or_else(|| anyhow!("{} arg not found", ARG_CHANGESET))?;
    let hyper_cs_id = helpers::csid_resolve(&ctx, hyper_repo.clone(), hyper_cs_id).await?;

    let source_repos =
        find_source_repos_and_latest_synced_cs_ids(&ctx, &hyper_repo, hyper_cs_id, matches).await?;

    for (source_repo, source_cs_id) in source_repos {
        info!(ctx.logger(), "validating {}", source_repo.blob_repo.name());

        let (mover, reverse_mover) = get_mover_and_reverse_mover(&source_repo.blob_repo)?;
        verify_working_copy_inner(
            &ctx,
            &Source(source_repo.blob_repo),
            &Target(hyper_repo.clone()),
            source_cs_id,
            Target(hyper_cs_id),
            &mover,
            &reverse_mover,
            Default::default(), // Visit all prefixes
        )
        .await?;
    }

    Ok(())
}

async fn run<'a>(fb: FacebookInit, matches: &'a MononokeMatches<'_>) -> Result<(), Error> {
    match matches.subcommand() {
        (SUBCOMMAND_ADD_SOURCE_REPO, Some(sub_m)) => {
            subcommand_add_source_repo(fb, matches, sub_m).await
        }
        (SUBCOMMAND_TAIL, Some(sub_m)) => subcommand_tail(fb, matches, sub_m).await,
        (SUBCOMMAND_VALIDATE, Some(sub_m)) => subcommand_validate(fb, matches, sub_m).await,
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
                )
                .arg(
                    Arg::with_name(ARG_PER_COMMIT_FILE_CHANGES_LIMIT)
                        .long(ARG_PER_COMMIT_FILE_CHANGES_LIMIT)
                        .required(false)
                        .takes_value(true)
                        .help("limits how many commits created in the initial commit that introduces the source repo")
                ),
        )
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_TAIL)
                .about("Tail source repos into hyper repo")
                .arg(
                    Arg::with_name(ARG_ONCE)
                        .long(ARG_ONCE)
                        .required(false)
                        .takes_value(false)
                        .help("Loop only once"),
                ),
        )
        .subcommand(
            SubCommand::with_name(SUBCOMMAND_VALIDATE)
                .about("Validate that files in hyper repo are identical to corresponding files in source repo")
                .arg(
                    Arg::with_name(ARG_CHANGESET)
                        .required(true)
                        .takes_value(true)
                        .help("bonsai/hg cs id or bookmark in hyper repo"),
                ),
        )
        .get_matches(fb)?;

    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(run(fb, &matches))
}
