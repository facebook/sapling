/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::bail;
use context::CoreContext;
use megarepolib::common::StackPosition;
use megarepolib::common::create_and_save_bonsai;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::RepoArgs;

use super::common::LightResultingChangesetArgs;
use super::common::get_commit_factory;

/// Create a bonsai merge commit
#[derive(Debug, clap::Args)]
pub struct BonsaiMergeArgs {
    #[clap(flatten)]
    pub repo_args: RepoArgs,

    #[command(flatten)]
    pub res_cs_args: LightResultingChangesetArgs,

    /// Two commits to merge
    #[clap(flatten)]
    pub commits: ChangesetArgs,
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: BonsaiMergeArgs) -> Result<()> {
    let repo: Repo = app.open_repo(&args.repo_args).await?;
    let changesets = args.commits.resolve_changesets(ctx, &repo).await?;
    if changesets.len() != 2 {
        bail!(
            "bonsai-merge requires exactly two commits, got {}",
            changesets.len()
        );
    }

    let cs_args_factory = get_commit_factory(args.res_cs_args, |s, _num| s.to_string())?;
    let cs_args = cs_args_factory(StackPosition(0));

    let merge_cs_id =
        create_and_save_bonsai(ctx, &repo, changesets, Default::default(), cs_args).await?;

    println!("{}", merge_cs_id);
    Ok(())
}
