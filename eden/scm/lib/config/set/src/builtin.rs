/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub(crate) fn get(name: &str) -> Option<&'static str> {
    // "%include builtin:git.rc" is no longer used. The check here is to avoid
    // a filesystem lookup.
    //
    // Consider using static config instead. Example: D54218773.
    if name == "builtin:git.rc" {
        Some("")
    } else {
        None
    }
}
