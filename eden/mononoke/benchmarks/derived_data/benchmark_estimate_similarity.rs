/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(iter_intersperse)]

// This benchmark generates two single line text files, in various sizes.
// Their contents are half matching with the delta evenly distributed
// throughout the line, e.g. 111111 vs 121212

use std::time::Duration;

use anyhow::Error;
use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use fbinit::FacebookInit;
use inferred_copy_from::similarity::estimate_similarity;

pub const KB: usize = 1024;
pub const MB: usize = KB * 1024;

#[fbinit::main]
fn main(_fb: FacebookInit) -> Result<(), Error> {
    let mut c = Criterion::default()
        .measurement_time(Duration::from_secs(60))
        .sample_size(10);

    let mut group = c.benchmark_group("estimate_similarity");

    let text1 = std::iter::repeat_n(b'1', 4 * MB).collect::<Vec<_>>();
    let text2 = std::iter::repeat(b'1')
        .intersperse(b'2')
        .take(4 * MB)
        .collect::<Vec<_>>();
    for size in [64, KB, MB, 4 * MB] {
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            b.iter(|| estimate_similarity(&text1[..size], &text2[..size]));
        });
    }
    group.finish();

    c.final_summary();
    Ok(())
}
