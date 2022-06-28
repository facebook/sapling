/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::iter::repeat;
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
    let mut group = c.benchmark_group("parallel_same_blob_gets");

    for size in [128, 16 * KB, 512 * KB, 8 * MB] {
        for concurrency in [4, 16, 256] {
            group.throughput(Throughput::Bytes(size as u64 * concurrency as u64));
            group.bench_with_input(
                BenchmarkId::from_parameter(format!("{} x{}", size, concurrency)),
                &size,
                |b, &size| {
                    let mut block = vec![0; size];
                    thread_rng().fill(&mut block as &mut [u8]);

                    let block = BlobstoreBytes::from_bytes(block);
                    let key = format!("benchmark.{:x}", thread_rng().next_u64());
                    runtime.block_on(async {
                        blobstore
                            .put(&ctx, key.clone(), block)
                            .await
                            .expect("Put failed")
                    });
                    let keys = repeat(key).take(concurrency);
                    let test = |ctx: CoreContext, blobstore: Arc<dyn Blobstore>| {
                        let keys = keys.clone();
                        async move {
                            let futs: FuturesUnordered<_> = keys
                                .map(|key| {
                                    let ctx = &ctx;
                                    let blobstore = &blobstore;
                                    async move { blobstore.get(ctx, &key).await }
                                })
                                .collect();
                            futs.try_for_each(|_| async move { Ok(()) })
                                .await
                                .expect("Gets failed");
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
