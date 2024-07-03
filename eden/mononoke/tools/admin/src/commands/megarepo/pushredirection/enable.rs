/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use mononoke_app::MononokeApp;

#[derive(Args)]
pub(super) struct EnableArgs {}

pub(super) async fn enable(_ctx: &CoreContext, _app: MononokeApp, _args: EnableArgs) -> Result<()> {
    Ok(())
}
