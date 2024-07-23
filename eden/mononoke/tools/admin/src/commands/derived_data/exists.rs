/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use bulk_derivation::BulkDerivation;
use clap::Args;
use context::CoreContext;
use derived_data_manager::DerivedDataManager;
use mononoke_app::args::ChangesetArgs;
use mononoke_app::args::DerivedDataArgs;

use super::Repo;

#[derive(Args)]
pub(super) struct ExistsArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    #[clap(flatten)]
    derived_data_args: DerivedDataArgs,
}

pub(super) async fn exists(
    ctx: &CoreContext,
    repo: &Repo,
    manager: &DerivedDataManager,
    args: ExistsArgs,
) -> Result<()> {
    let cs_ids = args.changeset_args.resolve_changesets(ctx, repo).await?;
    let derived_data_type = args.derived_data_args.resolve_type()?;

    let pending = manager
        .pending(ctx, &cs_ids, None, derived_data_type)
        .await?;

    for cs_id in cs_ids {
        if pending.contains(&cs_id) {
            println!("Not Derived: {}", cs_id);
        } else {
            println!("Derived: {}", cs_id);
        }
    }

    Ok(())
}
