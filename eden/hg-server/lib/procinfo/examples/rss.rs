/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

fn main() {
    let bytes = std::env::args().map(|a| a.parse().unwrap_or(0)).sum();
    println!("Allocating {} bytes", bytes);
    let _vec = vec![3u8; bytes];
    {
        println!("Allocating another {} bytes", bytes);
        let _vec = vec![7u8; bytes];
        println!("Releasing {} bytes", bytes);
    }
    println!(
        "Max RSS: {} bytes (expected: around {} bytes)",
        procinfo::max_rss_bytes(),
        bytes * 2
    );
}
