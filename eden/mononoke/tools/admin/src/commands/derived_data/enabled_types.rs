/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod list;
mod set;
mod show;
mod unset;

use anyhow::Result;
use clap::Args;
use clap::Subcommand;
use context::CoreContext;
use mononoke_app::MononokeApp;

use self::list::ListArgs;
use self::list::list;
use self::set::SetArgs;
use self::set::set;
use self::show::ShowArgs;
use self::show::show;
use self::unset::UnsetArgs;
use self::unset::unset;

/// Inspect and manage the `enabled_derived_data_types` table via the
/// `EnabledDerivedDataTypes` facet.
#[derive(Args)]
pub(super) struct EnabledTypesArgs {
    #[clap(subcommand)]
    subcommand: EnabledTypesSubcommand,
}

#[derive(Subcommand)]
enum EnabledTypesSubcommand {
    /// Show the derived data types enabled for a repo.
    Show(ShowArgs),
    /// List enabled-type rows across all repos, optionally filtered by type.
    List(ListArgs),
    /// Mark a derived data type as enabled for a repo.
    Set(SetArgs),
    /// Mark a derived data type as disabled for a repo (delete its row).
    Unset(UnsetArgs),
}

pub(super) async fn enabled_types(
    ctx: &CoreContext,
    app: &MononokeApp,
    args: EnabledTypesArgs,
) -> Result<()> {
    match args.subcommand {
        EnabledTypesSubcommand::Show(args) => show(ctx, app, args).await,
        EnabledTypesSubcommand::List(args) => list(ctx, app, args).await,
        EnabledTypesSubcommand::Set(args) => set(ctx, app, args).await,
        EnabledTypesSubcommand::Unset(args) => unset(ctx, app, args).await,
    }
}
