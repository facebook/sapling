/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// File with an existing JK referenced via const -- should produce no lint errors

const MERGE_RESOLUTION: &str = "scm/mononoke:pushrebase_enable_merge_resolution";

fn check_feature() -> Result<bool, anyhow::Error> {
    justknobs::eval(MERGE_RESOLUTION, None, None)
}
