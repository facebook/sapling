/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZero;

use anyhow::Context;
use anyhow::Result;
use context::CoreContext;
use live_commit_sync_config::LiveCommitSyncConfig;
use megarepolib::common::ChangesetArgs as MegarepoNewChangesetArgs;
use megarepolib::common::StackPosition;
use megarepolib::perform_move;
use megarepolib::perform_stack_move;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::RepoArgs;
use mononoke_app::args::SourceRepoArgs;
use movers::get_small_to_large_mover;
use repo_identity::RepoIdentityRef;
use slog::info;

use super::common::ResultingChangesetArgs;
use super::common::get_live_commit_sync_config;

/// Create a move commit, using a provided spec
#[derive(Debug, clap::Args)]
pub struct MoveArgs {
    #[clap(flatten, help = "Repo containing the commit to be moved")]
    repo_args: RepoArgs,

    #[clap(
        flatten,
        help = "Use predefined mover for part of megarepo, coming from this repo"
    )]
    source_repo_args: SourceRepoArgs,

    #[clap(flatten)]
    move_parent_cs: ChangesetArgs,

    #[clap(
        long,
        help = "how many files a single commit moves (note - that might create a stack of move commits instead of just one)"
    )]
    max_num_of_moves_in_commit: Option<NonZero<u64>>,

    #[clap(
        long,
        help = "which mapping version to use when remapping from small to large repo"
    )]
    mapping_version_name: String,

    #[command(flatten)]
    pub res_cs_args: ResultingChangesetArgs,
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: MoveArgs) -> Result<()> {
    let repo: Repo = app.open_repo(&args.repo_args).await?;
    let source_repo: Repo = app.open_repo(&args.source_repo_args).await?;

    let move_parent_bcs_id = args.move_parent_cs.resolve_changeset(ctx, &repo).await?;

    let mapping_version = CommitSyncConfigVersion(args.mapping_version_name);

    let live_commit_sync_config = get_live_commit_sync_config(ctx, &app, &args.repo_args)
        .await
        .context("building live_commit_sync_config")?;

    let source_repo_id = source_repo.repo_identity().id();
    let commit_sync_config = live_commit_sync_config
        .get_commit_sync_config_by_version(source_repo_id, &mapping_version)
        .await?;
    let mover = get_small_to_large_mover(&commit_sync_config, source_repo_id).unwrap();

    let new_cs_args: MegarepoNewChangesetArgs = args.res_cs_args.try_into()?;

    if let Some(max_num_of_moves_in_commit) = args.max_num_of_moves_in_commit {
        let changesets = perform_stack_move(
            ctx,
            &repo,
            move_parent_bcs_id,
            mover.as_ref(),
            max_num_of_moves_in_commit,
            |num: StackPosition| {
                let mut args = new_cs_args.clone();
                let message = args.message + &format!(" #{}", num.0);
                args.message = message;
                args
            },
        )
        .await?;
        info!(
            ctx.logger(),
            "created {} commits, with the last commit {:?}",
            changesets.len(),
            changesets.last()
        );
    } else {
        perform_move(ctx, &repo, move_parent_bcs_id, mover.as_ref(), new_cs_args).await?;
    }
    Ok(())
}
