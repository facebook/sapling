/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{iter::repeat, sync::Arc};

use criterion::{BenchmarkId, Criterion, Throughput};
use futures::{
    compat::Future01CompatExt,
    stream::{FuturesUnordered, TryStreamExt},
};
use rand::{thread_rng, Rng, RngCore};
use tokio_compat::runtime::Runtime;

use blobstore::{Blobstore, BlobstoreBytes};
use context::CoreContext;

use crate::{KB, MB};

pub fn benchmark(
    c: &mut Criterion,
    ctx: CoreContext,
    blobstore: Arc<dyn Blobstore>,
    runtime: &mut Runtime,
) {
    let mut group = c.benchmark_group("parallel_same_blob_gets");

    for size in [128, 16 * KB, 512 * KB, 8 * MB].iter() {
        for concurrency in [4, 16, 256].iter() {
            group.throughput(Throughput::Bytes(*size as u64 * *concurrency as u64));
            group.bench_with_input(
                BenchmarkId::from_parameter(format!("{} x{}", size, concurrency)),
                size,
                |b, &size| {
                    let mut block = Vec::with_capacity(size);
                    block.resize(size, 0u8);
                    thread_rng().fill(&mut block as &mut [u8]);

                    let block = BlobstoreBytes::from_bytes(block);
                    let key = format!("benchmark.{:x}", thread_rng().next_u64());
                    runtime.block_on_std(async {
                        blobstore
                            .put(ctx.clone(), key.clone(), block)
                            .compat()
                            .await
                            .expect("Put failed")
                    });
                    let keys = repeat(key).take(*concurrency);
                    let test = |ctx: CoreContext, blobstore: Arc<dyn Blobstore>| {
                        let keys = keys.clone();
                        async move {
                            let futs: FuturesUnordered<_> = keys
                                .map(|key| blobstore.get(ctx.clone(), key).compat())
                                .collect();
                            futs.try_for_each(|_| async move { Ok(()) })
                                .await
                                .expect("Gets failed");
                        }
                    };
                    b.iter(|| {
                        runtime
                            .block_on_std(async { test(ctx.clone(), Arc::clone(&blobstore)).await })
                    });
                },
            );
        }
    }

    group.finish();
}
