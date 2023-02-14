/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Args;
use context::CoreContext;
use mutable_counters::MutableCountersRef;

use super::Repo;

#[derive(Args)]
pub struct GetArgs {
    /// The name of the counter
    counter_name: String,
}

pub async fn get(ctx: &CoreContext, repo: &Repo, get_args: GetArgs) -> Result<()> {
    let mutable_counters = repo.mutable_counters();
    let maybe_value = mutable_counters
        .get_counter(ctx, get_args.counter_name.as_ref())
        .await?;
    println!("{:?}", maybe_value);
    Ok(())
}
