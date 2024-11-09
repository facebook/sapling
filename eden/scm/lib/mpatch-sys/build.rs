/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

fn main() {
    cc::Build::new()
        .file("../../sapling/mpatch.c")
        .include("../../../../")
        .compile("mpatch");
}
