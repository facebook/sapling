/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

fn main() {
    println!(
        "This host is {}",
        if hostcaps::is_prod() { "prod" } else { "corp" }
    );
}
