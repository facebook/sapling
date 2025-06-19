/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use cmdlib_cross_repo::create_single_direction_commit_syncer;
use context::CoreContext;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::SourceAndTargetRepoArgs;
use mononoke_types::NonRootMPath;

/// Run mover of a given version to remap paths between source and target repos
#[derive(Debug, clap::Args)]
pub struct RunMoverArgs {
    #[clap(flatten)]
    repo_args: SourceAndTargetRepoArgs,

    #[clap(long, help = "A version to use")]
    version: String,

    #[clap(long, help = "A path to remap")]
    path: String,
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: RunMoverArgs) -> Result<()> {
    let source_repo: Repo = app.open_repo(&args.repo_args.source_repo).await?;
    let target_repo: Repo = app.open_repo(&args.repo_args.target_repo).await?;
    let commit_sync_data =
        create_single_direction_commit_syncer(ctx, &app, source_repo, target_repo).await?;
    let version = CommitSyncConfigVersion(args.version);
    let movers = commit_sync_data.get_movers_by_version(&version).await?;
    let path = NonRootMPath::new(args.path)?;
    println!("{:?}", movers.mover.move_path(&path));
    Ok(())
}
