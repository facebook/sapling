/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use check_git_wc::check_git_wc;
use clap_old::Arg;
use cmdlib::args;
use cmdlib::args::MononokeClapApp;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers::block_execute;
use context::CoreContext;
use fbinit::FacebookInit;
use git2::Repository;
use git2::RepositoryOpenFlags;
use mononoke_types::ChangesetId;
use std::str::FromStr;

const ARG_CS_ID: &str = "csid";
const ARG_GIT_REPO_PATH: &str = "git-repo-path";
const ARG_GIT_COMMIT: &str = "git-commit";
const ARG_GIT_LFS: &str = "git-lfs";
const ARG_SCHEDULED_MAX: &str = "scheduled-max";

fn setup_app<'a, 'b>() -> MononokeClapApp<'a, 'b> {
    args::MononokeAppBuilder::new("Check that a working copy will match a git checkout")
        .build()
        .about("Check that a working copy for a given Bonsai is a perfect match to a git commit")
        .arg(
            Arg::with_name(ARG_CS_ID)
                .long(ARG_CS_ID)
                .value_name("BONSAI")
                .required(true)
                .help("Bonsai changeset whose working copy should be verified"),
        )
        .arg(
            Arg::with_name(ARG_GIT_REPO_PATH)
                .long(ARG_GIT_REPO_PATH)
                .value_name("PATH")
                .required(true)
                .help("Path to the git repo to compare to"),
        )
        .arg(
            Arg::with_name(ARG_GIT_COMMIT)
                .long(ARG_GIT_COMMIT)
                .value_name("HASH")
                .required(true)
                .help("The git commit to compare to"),
        )
        .arg(
            Arg::with_name(ARG_GIT_LFS)
                .long(ARG_GIT_LFS)
                .help("Enable git-lfs pointer parsing"),
        )
        .arg(
            Arg::with_name(ARG_SCHEDULED_MAX)
                .long(ARG_SCHEDULED_MAX)
                .takes_value(true)
                .required(false)
                .help("Maximum number of directories to check in parallel. Default 1"),
        )
}

async fn run_check_git_wc(
    fb: FacebookInit,
    ctx: &CoreContext,
    matches: &MononokeMatches<'_>,
) -> Result<()> {
    let cs = ChangesetId::from_str(matches.value_of(ARG_CS_ID).expect("Need Bonsai CS"))?;

    let git_commit = matches
        .value_of(ARG_GIT_COMMIT)
        .expect("Need git commit")
        .to_string();
    let git_lfs = matches.is_present(ARG_GIT_LFS);
    let git_repo = Repository::open_ext(
        matches
            .value_of(ARG_GIT_REPO_PATH)
            .expect("Need git repo path"),
        RepositoryOpenFlags::NO_SEARCH | RepositoryOpenFlags::BARE | RepositoryOpenFlags::NO_DOTGIT,
        std::iter::empty::<std::ffi::OsString>(),
    )?;

    let blobrepo = args::open_repo(fb, ctx.logger(), matches).await?;
    let scheduled_max = args::get_usize_opt(matches, ARG_SCHEDULED_MAX).unwrap_or(100) as usize;

    check_git_wc(
        ctx,
        &blobrepo,
        cs,
        git_repo,
        git_commit,
        git_lfs,
        scheduled_max,
    )
    .await
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let matches = setup_app().get_matches(fb)?;

    let logger = matches.logger();
    let ctx = CoreContext::new_with_logger(fb, logger.clone());

    block_execute(
        run_check_git_wc(fb, &ctx, &matches),
        fb,
        "check_git_wc",
        logger,
        &matches,
        cmdlib::monitoring::AliveService,
    )
}
