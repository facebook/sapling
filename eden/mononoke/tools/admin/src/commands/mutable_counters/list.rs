/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use context::CoreContext;
use mutable_counters::MutableCountersRef;

use super::Repo;

pub async fn list(ctx: &CoreContext, repo: &Repo) -> Result<()> {
    let mutable_counters = repo.mutable_counters();
    let counters = mutable_counters.get_all_counters(ctx).await?;

    for (name, value) in counters {
        println!("{:<30}={}", name, value);
    }

    Ok(())
}
