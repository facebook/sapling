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
use phases::PhasesRef;

use super::Repo;

#[derive(Args)]
pub(super) struct FetchArgs {
    #[clap(flatten)]
    changeset_args: ChangesetArgs,
}

pub(super) async fn fetch(ctx: &CoreContext, repo: &Repo, args: FetchArgs) -> Result<()> {
    let cs_id = args.changeset_args.resolve_changeset(ctx, repo).await?;

    let public_phases = repo.phases().get_public(ctx, vec![cs_id], false).await?;

    if public_phases.contains(&cs_id) {
        println!("public");
    } else {
        println!("draft");
    }

    Ok(())
}
