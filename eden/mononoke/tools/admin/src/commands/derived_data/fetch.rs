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
pub(super) struct FetchArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,

    #[clap(flatten)]
    derived_data_args: DerivedDataArgs,
}

pub(super) async fn fetch(
    ctx: &CoreContext,
    repo: &Repo,
    manager: &DerivedDataManager,
    args: FetchArgs,
) -> Result<()> {
    let cs_ids = args.changeset_args.resolve_changesets(ctx, repo).await?;
    let derived_data_type = args.derived_data_args.resolve_type()?;

    let derived =
        BulkDerivation::fetch_derived_batch(manager, ctx, &cs_ids, None, derived_data_type).await?;

    for cs_id in cs_ids {
        if let Some(derived) = derived.get(&cs_id) {
            println!("Derived: {} -> {}", cs_id, derived);
        } else {
            println!("Not Derived: {}", cs_id);
        }
    }

    Ok(())
}
