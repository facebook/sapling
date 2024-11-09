/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

fn main() {
    #[cfg(target_os = "macos")]
    {
        cc::Build::new().file("darwin.c").compile("darwin");
    }
}
