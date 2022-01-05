/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;

fn main() -> Result<()> {
    for arg in std::env::args().skip(1) {
        println!("{}: {}", &arg, fsinfo::fstype(&arg)?.to_string());
    }
    Ok(())
}
