/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::str::FromStr;

use anyhow::Result;
use check_git_wc::check_git_wc;
use clap::Parser;
use context::CoreContext;
use fbinit::FacebookInit;
use git2::Repository;
use git2::RepositoryOpenFlags;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use mononoke_app::args::RepoArgs;
use mononoke_app::monitoring::AliveService;
use mononoke_app::monitoring::MonitoringAppExtension;
use mononoke_types::ChangesetId;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;

#[facet::container]
struct HgRepo {
    #[facet]
    repo_identity: RepoIdentity,

    #[facet]
    repo_blobstore: RepoBlobstore,

    #[facet]
    repo_derived_data: RepoDerivedData,
}

/// Validate the working copy in Git
#[derive(Parser)]
#[clap(about = "Tool for checking that a working copy will match a git checkout.")]
struct CheckGitWCArgs {
    /// The repo against which the command needs to be executed
    #[clap(flatten)]
    repo: RepoArgs,
    /// Bonsai changeset whose working copy should be verified
    #[clap(long, value_name = "BONSAI")]
    csid: String,
    /// Path to the git repo to compare to
    #[clap(long, value_name = "PATH")]
    git_repo_path: String,
    /// The git commit to compare to
    #[clap(long, value_name = "HASH")]
    git_commit: String,
    /// Enable git-lfs pointer parsing
    #[clap(long)]
    git_lfs: bool,
    /// Maximum number of directories to check in parallel. Default 100
    #[clap(long, default_value_t = 100)]
    scheduled_max: usize,
}

async fn run_check_git_wc(app: MononokeApp) -> Result<()> {
    let logger = app.logger();
    let ctx = CoreContext::new_with_logger(app.fb, logger.clone());

    let args: CheckGitWCArgs = app.args()?;
    let cs = ChangesetId::from_str(&args.csid)?;

    let git_repo = Repository::open_ext(
        args.git_repo_path,
        RepositoryOpenFlags::NO_SEARCH | RepositoryOpenFlags::BARE | RepositoryOpenFlags::NO_DOTGIT,
        std::iter::empty::<std::ffi::OsString>(),
    )?;

    let hg_repo: HgRepo = app.open_repo(&args.repo).await?;

    check_git_wc(
        &ctx,
        &hg_repo,
        cs,
        git_repo,
        args.git_commit,
        args.git_lfs,
        args.scheduled_max,
    )
    .await
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = MononokeAppBuilder::new(fb)
        .with_app_extension(MonitoringAppExtension {})
        .build::<CheckGitWCArgs>()?;
    app.run_with_monitoring_and_logging(run_check_git_wc, "check_git_wc", AliveService)
}
