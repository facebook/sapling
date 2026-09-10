/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

fn main() {
    let mut build = cc::Build::new();
    build
        .file("native/sigbus_memops.c")
        .file("native/sigbus_memops_inline.c")
        .include(".")
        .include("native");

    build.compile("sigbus_memops");
}
