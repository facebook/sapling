/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

fn main() {
    println!("cargo:rerun-if-env-changed=SAPLING_VERSION");
    println!("cargo:rerun-if-env-changed=SAPLING_VERSION_HASH");
}
