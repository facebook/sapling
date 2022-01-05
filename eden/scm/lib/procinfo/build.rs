/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

fn main() {
    #[cfg(target_os = "macos")]
    {
        cc::Build::new().file("darwin.c").compile("darwin");
    }
}
