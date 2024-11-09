/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;

fn main() -> Result<()> {
    for arg in std::env::args().skip(1) {
        println!("{}: {}", &arg, fsinfo::fstype(&arg)?);
    }
    Ok(())
}
