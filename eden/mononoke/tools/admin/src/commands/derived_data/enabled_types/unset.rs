/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use clap::Args;
use context::CoreContext;
use enabled_derived_data_types::EnabledDerivedDataTypesRef;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArgs;
use mononoke_types::DerivableType;
use repo_identity::RepoIdentityRef;

use super::super::Repo;

#[derive(Args)]
pub(super) struct UnsetArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    /// Derived data type to mark disabled for this repo.
    #[clap(short = 'T', long)]
    derived_data_type: String,

    #[clap(long)]
    i_know_what_i_am_doing: bool,
}

pub(super) async fn unset(ctx: &CoreContext, app: &MononokeApp, args: UnsetArgs) -> Result<()> {
    if !args.i_know_what_i_am_doing {
        return Err(Error::msg(
            "marking a derived data type disabled writes config-like state that \
            gates derivation.\nIf you still want to proceed, re-run the 'unset' \
            command with '--i-know-what-i-am-doing' flag to unblock.",
        ));
    }

    let derived_data_type = DerivableType::from_name(&args.derived_data_type)?;
    let repo: Repo = app.open_repo(&args.repo).await?;
    let repo_id = repo.repo_identity().id();

    repo.enabled_derived_data_types()
        .mark_disabled(ctx, repo_id, derived_data_type)
        .await?;

    Ok(())
}
