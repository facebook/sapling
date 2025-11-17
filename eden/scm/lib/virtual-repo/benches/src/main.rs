/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Benchmark for virtual-tree algorithms.

use minibench::bench;
use minibench::elapsed;
use virtual_repo::text_gen::generate_file_content_of_length;
use virtual_repo::text_gen::generate_paragraphs;

fn main() {
    for (size, n) in [(3000, 10000), (10_000_000, 1)] {
        bench(
            format!("generate_file_content_of_length({size}) {n} times"),
            || {
                elapsed(|| {
                    for _ in 0..n {
                        let _content = generate_file_content_of_length(size);
                    }
                })
            },
        );
    }

    // Warm-up (load and analyse corpus).
    let _ = generate_paragraphs(1, 0);
    for (size, n) in [(3000, 1000), (10_000_000, 1)] {
        bench(format!("generate_paragraphs({size}) {n} times"), || {
            elapsed(|| {
                for _ in 0..n {
                    let _content = generate_paragraphs(size, 42);
                }
            })
        });
    }
}
