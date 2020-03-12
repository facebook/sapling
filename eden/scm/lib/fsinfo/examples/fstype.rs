/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

fn main() {
    for arg in std::env::args().skip(1) {
        println!("{}: {}", &arg, fsinfo::fstype(&arg).unwrap());
    }
}
