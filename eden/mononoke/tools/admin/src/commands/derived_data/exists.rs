/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use mononoke_app::args::ChangesetArgs;

use super::args::DerivedUtilsArgs;
use super::Repo;

#[derive(Args)]
pub(super) struct ExistsArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,
    #[clap(flatten)]
    derived_utils_args: DerivedUtilsArgs,
}

pub(super) async fn exists(ctx: &CoreContext, repo: &Repo, args: ExistsArgs) -> Result<()> {
    let derived_utils = args.derived_utils_args.derived_utils(ctx, repo)?;

    let cs_ids = args.changeset_args.resolve_changesets(ctx, repo).await?;

    let pending = derived_utils
        .pending(ctx.clone(), repo.repo_derived_data.clone(), cs_ids.clone())
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
