/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap;
use context::CoreContext;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use regex::Regex;

use super::catchup;

/// Validate invariants about the catchup
#[derive(Debug, clap::Args)]
pub struct CatchupValidateArgs {
    #[clap(flatten)]
    pub repo_args: RepoArgs,

    /// Merge commit i.e. commit where all catchup commits were merged into
    #[clap(long = "commit-hash")]
    commit_hash: String,

    /// Commit to merge
    #[clap(long = "to-merge-cs-id")]
    to_merge_cs_id: String,

    /// Regex that matches all paths that should be merged in head commit
    #[clap(long = "path-regex")]
    path_regex: String,
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: CatchupValidateArgs) -> Result<()> {
    let repo: Repo = app.open_repo(&args.repo_args).await?;
    let path_regex = Regex::new(&args.path_regex)?;

    let head_commit = args.commit_hash.parse()?;
    let to_merge_commit = args.to_merge_cs_id.parse()?;

    catchup::validate(ctx, &repo, head_commit, to_merge_commit, path_regex).await
}
