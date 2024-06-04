/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;

use super::Repo;

#[derive(Args)]
/// Subcommand to describe data associated with a given hgid (tree/file) within the cas store.
pub struct CasStoreInfoArgs {}

pub async fn cas_store_info(
    _ctx: &CoreContext,
    _repo: &Repo,
    _args: CasStoreInfoArgs,
) -> Result<()> {
    Ok(())
}
