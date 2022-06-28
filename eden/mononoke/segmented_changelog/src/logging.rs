/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use scuba_ext::MononokeScubaSampleBuilder;

use context::CoreContext;
use mononoke_types::RepositoryId;

use crate::types::IdDagVersion;
use crate::types::SegmentedChangelogVersion;

const SCUBA_TABLE: &str = "segmented_changelog_version";

pub fn log_new_iddag_version(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    iddag_version: IdDagVersion,
) {
    if let Some(mut sample_builder) = new_sample_builder(ctx) {
        sample_builder
            .add("type", "iddag")
            .add("repo_id", repo_id.id())
            .add("iddag_version", format!("{}", iddag_version.0))
            .log(); // note that logging may fail
    }
}

pub fn log_new_segmented_changelog_version(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    sc_version: SegmentedChangelogVersion,
) {
    slog::info!(
        ctx.logger(),
        "segmented changelog version saved, idmap_version: {}, iddag_version: {}",
        sc_version.idmap_version,
        sc_version.iddag_version,
    );
    if let Some(mut sample_builder) = new_sample_builder(ctx) {
        sample_builder
            .add("type", "segmented_changelog")
            .add("repo_id", repo_id.id())
            .add("idmap_version", sc_version.idmap_version.0)
            .add("iddag_version", format!("{}", sc_version.iddag_version.0))
            .log(); // note that logging may fail
    }
}

fn new_sample_builder(ctx: &CoreContext) -> Option<MononokeScubaSampleBuilder> {
    // We construct a completely new scuba sample builder to log to the version scuba table so we
    // check the context to verify if we are in an environment where we are allowed to log.
    // We want to avoid logging from tests.
    if ctx.scuba().is_discard() {
        return None;
    }
    let mut sample_builder = MononokeScubaSampleBuilder::new(ctx.fb, SCUBA_TABLE);
    sample_builder.add_common_server_data();
    Some(sample_builder)
}
