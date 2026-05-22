/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// File with only existing JK references -- should produce no lint errors

fn check_feature() -> Result<bool, anyhow::Error> {
    justknobs::eval(
        "scm/mononoke:pushrebase_enable_merge_resolution",
        None,
        None,
    )
}
