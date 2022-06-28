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
use futures::stream::FuturesUnordered;
use futures::stream::TryStreamExt;
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
    let mut group = c.benchmark_group("parallel_puts");

    for size in [128, 16 * KB, 512 * KB, 8 * MB] {
        for concurrency in [4, 16, 256] {
            group.throughput(Throughput::Bytes(size as u64 * concurrency as u64));
            group.bench_with_input(
                BenchmarkId::from_parameter(format!("{} x{}", size, concurrency)),
                &size,
                |b, &size| {
                    let blocks: Vec<_> = std::iter::repeat(())
                        .take(concurrency)
                        .map(|()| {
                            let mut block = vec![0; size];
                            thread_rng().fill(&mut block as &mut [u8]);

                            BlobstoreBytes::from_bytes(block)
                        })
                        .collect();
                    let test = |ctx: CoreContext, blobstore: Arc<dyn Blobstore>| {
                        let blocks = blocks.clone();
                        async move {
                            let futs: FuturesUnordered<_> = blocks
                                .into_iter()
                                .map(|block| {
                                    let key = format!("benchmark.{:x}", thread_rng().next_u64());
                                    blobstore.put(&ctx, key, block)
                                })
                                .collect();

                            futs.try_for_each(|_| async move { Ok(()) })
                                .await
                                .expect("Puts failed");
                        }
                    };
                    b.iter(|| {
                        runtime.block_on(async { test(ctx.clone(), Arc::clone(&blobstore)).await })
                    });
                },
            );
        }
    }
    group.finish();
}
