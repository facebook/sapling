/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use cmdlib_cross_repo::create_single_direction_commit_syncer;
use context::CoreContext;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::MononokeApp;

use super::Repo;

/// Query cross-repo commit mapping
#[derive(Args)]
pub struct MapArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,
}

pub async fn map(
    ctx: &CoreContext,
    app: &MononokeApp,
    source_repo: Repo,
    target_repo: Repo,
    args: MapArgs,
) -> Result<()> {
    let source_cs_id = args
        .changeset_args
        .resolve_changeset(ctx, &source_repo)
        .await?;

    let commit_syncer =
        create_single_direction_commit_syncer(ctx, app, source_repo, target_repo).await?;

    let plural_commit_sync_outcome = commit_syncer
        .get_plural_commit_sync_outcome(ctx, source_cs_id)
        .await?;
    match plural_commit_sync_outcome {
        Some(plural_commit_sync_outcome) => {
            println!("{:?}", plural_commit_sync_outcome);
        }
        None => {
            println!("{} is not remapped", source_cs_id);
        }
    }

    Ok(())
}
