/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use scuba_ext::ScubaSampleBuilder;

const SCUBA_TABLE: &str = "mononoke_x_repo_mapping";

pub fn get_scuba_sample(ctx: &CoreContext) -> ScubaSampleBuilder {
    let mut scuba_sample = ScubaSampleBuilder::new(ctx.fb, SCUBA_TABLE);
    scuba_sample.add_common_server_data();
    scuba_sample
}
