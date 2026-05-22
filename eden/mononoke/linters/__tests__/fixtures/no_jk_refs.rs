/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

fn example() -> Result<(), anyhow::Error> {
    let x = 42;
    println!("no justknobs references here: {}", x);
    Ok(())
}
