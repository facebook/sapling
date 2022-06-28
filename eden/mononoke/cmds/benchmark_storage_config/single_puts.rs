/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use criterion::BenchmarkId;
use criterion::Criterion;
use criterion::Throughput;
use rand::thread_rng;
use rand::Rng;
use rand::RngCore;
use tokio::runtime::Handle;

use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use context::CoreContext;

use crate::KB;
use crate::MB;

pub fn benchmark(
    c: &mut Criterion,
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    runtime: &Handle,
) {
    let mut group = c.benchmark_group("single_puts");

    for size in [128, 16 * KB, 512 * KB, 8 * MB] {
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let mut block = vec![0; size];
            thread_rng().fill(&mut block as &mut [u8]);

            let block = BlobstoreBytes::from_bytes(block);
            let test = |ctx, blobstore: Arc<dyn Blobstore>| {
                let block = block.clone();
                async move {
                    let key = format!("benchmark.{:x}", thread_rng().next_u64());
                    blobstore.put(&ctx, key, block).await.expect("Put failed");
                }
            };
            b.iter(|| runtime.block_on(async { test(ctx.clone(), Arc::clone(&blobstore)).await }));
        });
    }
    group.finish();
}
