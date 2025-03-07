/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use context::CoreContext;
use metadata::Metadata;
use mononoke_app::MononokeApp;
use scuba_ext::MononokeScubaSampleBuilder;

pub(crate) fn new(
    app: Arc<MononokeApp>,
    metadata: &Metadata,
    repo_name: &str,
    dry_run: bool,
) -> Result<MononokeScubaSampleBuilder> {
    let uuid = uuid::Uuid::new_v4();
    let mut scuba_sample = app.environment().scuba_sample_builder.clone();
    scuba_sample.add_metadata(metadata);
    scuba_sample.add("run_id", uuid.to_string());
    scuba_sample.add("repo", repo_name);
    scuba_sample.add("dry_run", dry_run);

    Ok(scuba_sample)
}

pub(crate) fn log_sync_start(ctx: &CoreContext, start_id: u64) -> Result<bool> {
    let mut scuba_sample = ctx.scuba().clone();

    scuba_sample.add("log_tag", "Start sync process");
    scuba_sample.add("start_id", start_id);

    Ok(scuba_sample.log())
}
