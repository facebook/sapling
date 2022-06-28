/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use criterion::Criterion;
use tokio::runtime::Runtime;

use context::CoreContext;
use mononoke_types::hash::Blake2;
use mononoke_types::ChangesetId;
use segmented_changelog::ConcurrentMemIdMap;
use segmented_changelog::DagId;
use segmented_changelog::IdMap;

async fn insert(ctx: &CoreContext, idmap: &dyn IdMap, low: u64, high: u64) {
    let mut cs_id_bytes = [0u8; 32];
    for i in low..high {
        cs_id_bytes[0] = (i & 255) as u8;
        cs_id_bytes[1] = (i >> 8) as u8;
        let cs_id = ChangesetId::from(Blake2::from_byte_array(cs_id_bytes.clone()));
        // no good reason to join these futures
        idmap
            .insert(ctx, DagId(i), cs_id)
            .await
            .expect("failed to insert");
    }
}

async fn find(ctx: &CoreContext, idmap: &dyn IdMap, v: DagId) {
    let _ = idmap
        .find_changeset_id(ctx, v)
        .await
        .expect("failed_find_changeset_id");
}

fn insert_benchmark(c: &mut Criterion, runtime: &Runtime, ctx: &CoreContext) {
    c.bench_function("insert 0..10000", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let idmap = ConcurrentMemIdMap::new();
                insert(ctx, &idmap, 0, 10000).await;
            });
        })
    });
}

fn read_benchmark(c: &mut Criterion, runtime: &Runtime, ctx: &CoreContext) {
    let idmap = ConcurrentMemIdMap::new();
    runtime.block_on(async {
        insert(ctx, &idmap, 0, 10000).await;
    });
    c.bench_function("read 0..10000", |b| {
        b.iter(|| {
            runtime.block_on(async {
                for chunk in (0..10000u64).collect::<Vec<_>>().chunks(1000) {
                    let mut f = vec![];
                    for j in chunk {
                        f.push(find(ctx, &idmap, DagId(*j)));
                    }
                    let _ = futures::future::join_all(f).await;
                }
            });
        })
    });
}

fn read_write_benchmark(c: &mut Criterion, runtime: &Runtime, ctx: &CoreContext) {
    let idmap = ConcurrentMemIdMap::new();
    let mx = 10000;
    runtime.block_on(async {
        insert(ctx, &idmap, 0, mx).await;
    });
    c.bench_function("read 0..10000; write 1..100", |b| {
        b.iter(|| {
            runtime.block_on(async {
                for i in 0..100 {
                    let mut read = vec![];
                    for j in 0..100 {
                        read.push(find(ctx, &idmap, DagId(i * 100 + j)));
                    }
                    let _ = futures::future::join(
                        futures::future::join_all(read),
                        insert(ctx, &idmap, mx + i, mx + i + 1),
                    )
                    .await;
                }
            });
        })
    });
}

#[fbinit::main]
fn main(fb: fbinit::FacebookInit) {
    let runtime = Runtime::new().expect("failed to initialize runtime");
    let ctx = CoreContext::test_mock(fb);

    let mut criterion = Criterion::default().sample_size(10);

    insert_benchmark(&mut criterion, &runtime, &ctx);
    read_benchmark(&mut criterion, &runtime, &ctx);
    read_write_benchmark(&mut criterion, &runtime, &ctx);

    criterion.final_summary();
}
