/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

fn suppressed() -> Result<bool, anyhow::Error> {
    // @lint-ignore RUSTJKEXISTS
    justknobs::eval("scm/mononoke:this_knob_does_not_exist", None, None)
}

fn not_suppressed() -> Result<bool, anyhow::Error> {
    justknobs::eval("scm/mononoke:this_knob_does_not_exist", None, None)
}
