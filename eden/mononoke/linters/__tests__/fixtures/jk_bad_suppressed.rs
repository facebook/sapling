/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// File with a non-existent JK reference suppressed via lint-ignore -- should produce no lint error

fn check_feature() -> Result<bool, anyhow::Error> {
    // @lint-ignore RUSTJKEXISTS
    justknobs::eval("scm/mononoke:this_knob_does_not_exist", None, None)
}
