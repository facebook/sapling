/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod backfill_noop_mapping;
mod bonsai_merge;
pub mod check_prereqs;
pub(crate) mod common;
mod gradual_delete;
mod merge;
mod move_commit;
mod pre_merge_delete;
mod pushredirection;
mod run_mover;
mod sync_diamond_merge;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::MononokeApp;

use self::backfill_noop_mapping::BackfillNoopMappingArgs;
use self::bonsai_merge::BonsaiMergeArgs;
use self::check_prereqs::CheckPrereqsArgs;
use self::gradual_delete::GradualDeleteArgs;
use self::merge::MergeArgs;
use self::move_commit::MoveArgs;
use self::pre_merge_delete::PreMergeDeleteArgs;
use self::pushredirection::PushRedirectionArgs;
use self::run_mover::RunMoverArgs;
use self::sync_diamond_merge::SyncDiamondMergeArgs;

/// Manage megarepo
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(subcommand)]
    subcommand: MegarepoSubcommand,
}

#[derive(Subcommand)]
enum MegarepoSubcommand {
    BackfillNoopMapping(BackfillNoopMappingArgs),
    /// Manage which repos are pushredirected to the large repo
    PushRedirection(PushRedirectionArgs),
    Merge(MergeArgs),
    MoveCommit(MoveArgs),
    RunMover(RunMoverArgs),
    SyncDiamondMerge(SyncDiamondMergeArgs),
    GradualDelete(GradualDeleteArgs),
    PreMergeDelete(PreMergeDeleteArgs),
    BonsaiMerge(BonsaiMergeArgs),
    CheckPrereqs(CheckPrereqsArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    match args.subcommand {
        MegarepoSubcommand::BackfillNoopMapping(args) => {
            backfill_noop_mapping::run(&ctx, app, args).await?
        }
        MegarepoSubcommand::PushRedirection(args) => pushredirection::run(&ctx, app, args).await?,
        MegarepoSubcommand::Merge(args) => merge::run(&ctx, app, args).await?,
        MegarepoSubcommand::MoveCommit(args) => move_commit::run(&ctx, app, args).await?,
        MegarepoSubcommand::RunMover(args) => run_mover::run(&ctx, app, args).await?,
        MegarepoSubcommand::SyncDiamondMerge(args) => {
            sync_diamond_merge::run(&ctx, app, args).await?
        }
        MegarepoSubcommand::PreMergeDelete(args) => pre_merge_delete::run(&ctx, app, args).await?,
        MegarepoSubcommand::BonsaiMerge(args) => bonsai_merge::run(&ctx, app, args).await?,
        MegarepoSubcommand::CheckPrereqs(args) => check_prereqs::run(&ctx, app, args).await?,
        MegarepoSubcommand::GradualDelete(args) => gradual_delete::run(&ctx, app, args).await?,
    }

    Ok(())
}
