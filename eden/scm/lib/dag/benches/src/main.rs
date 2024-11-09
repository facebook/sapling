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

use minibench::bench_enabled;

fn main() {
    if bench_enabled("dag_ops") {
        dag_ops::main();
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

// Supports turning on tracing via LOG=...
dev_logger::init!();
