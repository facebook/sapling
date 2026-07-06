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
pub(super) struct SetArgs {
    #[clap(flatten)]
    repo: RepoArgs,

    /// Derived data type to mark enabled for this repo.
    #[clap(short = 'T', long)]
    derived_data_type: String,

    /// Campaign (root request id) that enabled this type. Omit for a manual poke.
    #[clap(long)]
    root_request_id: Option<u64>,

    #[clap(long)]
    i_know_what_i_am_doing: bool,
}

pub(super) async fn set(ctx: &CoreContext, app: &MononokeApp, args: SetArgs) -> Result<()> {
    if !args.i_know_what_i_am_doing {
        return Err(Error::msg(
            "marking a derived data type enabled writes config-like state that \
            gates derivation.\nIf you still want to proceed, re-run the 'set' \
            command with '--i-know-what-i-am-doing' flag to unblock.",
        ));
    }

    let derived_data_type = DerivableType::from_name(&args.derived_data_type)?;
    let repo: Repo = app.open_repo(&args.repo).await?;
    let repo_id = repo.repo_identity().id();

    repo.enabled_derived_data_types()
        .mark_enabled(ctx, repo_id, derived_data_type, args.root_request_id)
        .await?;

    Ok(())
}
