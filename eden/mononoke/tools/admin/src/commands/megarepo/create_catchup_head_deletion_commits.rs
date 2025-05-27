/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bookmarks::BookmarkKey;
use clap;
use context::CoreContext;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::AsRepoArg;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::RepoArgs;
use regex::Regex;

use super::catchup;
use super::common::ResultingChangesetArgs;

/// Create delete commits for 'catchup strategy.
/// This is normally used after invisible merge is done, but small repo got a few new commits
/// that needs merging.
///
/// O         <-  head bookmark
/// |
/// O   O <-  new commits (we want to merge them in master)
/// |  ...
/// IM  |       <- invisible merge commit
/// |\\ /
/// O O
///
/// This command create deletion commits on top of master bookmark for files that were changed in new commits,
/// and pushrebases them.
///
/// After all of the commits are pushrebased paths that match --path-regex in head bookmark should be a subset
/// of all paths that match --path-regex in the latest new commit we want to merge.
#[derive(Debug, clap::Args)]
pub struct CreateCatchupHeadDeletionCommitsArgs {
    #[clap(flatten)]
    pub repo_args: RepoArgs,

    /// Bookmark name to merge into
    #[clap(long = "head-bookmark")]
    head_bookmark: String,

    /// Changeset to merge (could be a bookmark or a changeset id)
    #[clap(flatten)]
    to_merge_cs_id: ChangesetArgs,

    /// Regex that matches all paths that should be merged in head commit
    #[clap(long = "path-regex")]
    path_regex: String,

    /// How many files to delete in a single commit
    #[clap(long = "deletion-chunk-size", default_value = "10000")]
    deletion_chunk_size: Option<usize>,

    /// How many seconds to wait after each push
    #[clap(long = "wait-secs", default_value = "0")]
    wait_secs: Option<u64>,

    #[command(flatten)]
    pub res_cs_args: ResultingChangesetArgs,
}

pub async fn run(
    ctx: &CoreContext,
    app: MononokeApp,
    args: CreateCatchupHeadDeletionCommitsArgs,
) -> Result<()> {
    let repo: Repo = app.open_repo(&args.repo_args).await?;
    let head_bookmark = BookmarkKey::new(&args.head_bookmark)?;
    let path_regex = Regex::new(&args.path_regex)?;
    let deletion_chunk_size = args.deletion_chunk_size.unwrap_or(10000);
    let wait_secs = args.wait_secs.unwrap_or(0);
    let (_, repo_config) = app.repo_config(args.repo_args.as_repo_arg())?;

    catchup::create_deletion_head_commits(
        ctx,
        &repo,
        head_bookmark,
        args.to_merge_cs_id.resolve_changeset(ctx, &repo).await?,
        path_regex,
        deletion_chunk_size,
        args.res_cs_args,
        &repo_config.pushrebase.flags,
        wait_secs,
    )
    .await
}
