/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use phases::PhasesRef;

use super::Repo;

#[derive(Args)]
pub(super) struct ListPublicArgs {}

pub(super) async fn list_public(
    ctx: &CoreContext,
    repo: &Repo,
    _args: ListPublicArgs,
) -> Result<()> {
    let cs_ids = repo.phases().list_all_public(ctx).await?;
    for cs_id in cs_ids {
        println!("{}", cs_id);
    }
    Ok(())
}
