/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
