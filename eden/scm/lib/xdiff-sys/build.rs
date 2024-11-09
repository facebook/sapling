/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

fn main() {
    cc::Build::new()
        .files(&[
            "../third-party/xdiff/xdiffi.c",
            "../third-party/xdiff/xprepare.c",
            "../third-party/xdiff/xutils.c",
        ])
        .compile("xdiff");
}
