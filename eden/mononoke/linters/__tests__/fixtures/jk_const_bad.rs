/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// File with a non-existent JK referenced via const -- should produce a lint error

const MY_KNOB: &str = "scm/mononoke:this_knob_does_not_exist";

fn check_feature() -> Result<bool, anyhow::Error> {
    justknobs::eval(MY_KNOB, None, None)
}
