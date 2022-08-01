/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

fn main() {
    println!(
        "This host is {}",
        if hostcaps::is_prod() {
            "prod"
        } else if hostcaps::is_corp() {
            "corp"
        } else {
            "lab"
        }
    );
}
