/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use scuba::ScubaSampleBuilder;

use context::CoreContext;
use mononoke_types::RepositoryId;

use crate::types::{DagBundle, IdDagVersion, IdMapVersion};

const SCUBA_TABLE: &str = "segmented_changelog_version";

pub fn log_new_idmap_version(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    idmap_version: IdMapVersion,
) {
    ScubaSampleBuilder::new(ctx.fb, SCUBA_TABLE)
        .add_common_server_data()
        .add("type", "idmap")
        .add("repo_id", repo_id.id())
        .add("idmap_version", idmap_version.0)
        .log(); // note that logging may fail
}

pub fn log_new_iddag_version(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    iddag_version: IdDagVersion,
) {
    ScubaSampleBuilder::new(ctx.fb, SCUBA_TABLE)
        .add_common_server_data()
        .add("type", "iddag")
        .add("repo_id", repo_id.id())
        .add("iddag_version", format!("{}", iddag_version.0))
        .log(); // note that logging may fail
}

pub fn log_new_bundle(ctx: &CoreContext, repo_id: RepositoryId, bundle: DagBundle) {
    ScubaSampleBuilder::new(ctx.fb, SCUBA_TABLE)
        .add_common_server_data()
        .add("type", "bundle")
        .add("repo_id", repo_id.id())
        .add("idmap_version", bundle.idmap_version.0)
        .add("iddag_version", format!("{}", bundle.iddag_version.0))
        .log(); // note that logging may fail
}
