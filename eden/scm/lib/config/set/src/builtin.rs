/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
