/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod dag_import;
mod dag_ops;
mod idset;
mod inprocess_iddag_serde;
mod segment_sizes;

use dag::Vertex;
use dag::utils::break_parent_func_cycle;
use minibench::bench;
use minibench::bench_enabled;
use minibench::elapsed;

fn main() {
    if bench_enabled("cycle_break") {
        bench_cycle_break();
    }
    if bench_enabled("dag_ops") {
        dag_ops::main();
    }
    if bench_enabled("parents_single_segment") {
        dag_ops::bench_parents_single_segment();
    }
    if bench_enabled(
        "dag_import/clone_clone_data dag_import/import_clone_data dag_import/import_pull_data",
    ) {
        dag_import::main();
    }
    if bench_enabled("inprocess_iddag_serde") {
        inprocess_iddag_serde::main();
    }
    if bench_enabled("idset") {
        idset::main();
    }
    if bench_enabled("segment_sizes") {
        segment_sizes::main();
    }
}

fn bench_cycle_break() {
    for size in [1_000usize, 10_000] {
        let vertices: Vec<Vertex> = (0..size * 2)
            .map(|i| Vertex::copy_from(&i.to_le_bytes()))
            .collect();
        bench(format!("cycle_break/shared_ancestry_{size}"), || {
            elapsed(|| {
                let parents = break_parent_func_cycle(|v: Vertex| {
                    let id = usize::from_le_bytes(v.as_ref().try_into().unwrap());
                    Ok(if id == 0 {
                        Vec::new()
                    } else {
                        vec![vertices[id.min(size) - 1].clone()]
                    })
                });
                for v in vertices[..size].iter().rev().chain(&vertices[size..]) {
                    parents(v.clone()).unwrap();
                }
            })
        });
    }
}

// Supports turning on tracing via LOG=...
dev_logger::init!();
