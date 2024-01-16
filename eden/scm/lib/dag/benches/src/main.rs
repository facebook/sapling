/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod dag_ops;
mod inprocess_iddag_serde;
mod segment_sizes;
mod spanset;

use minibench::bench_enabled;

fn main() {
    if bench_enabled("dag_ops") {
        dag_ops::main();
    }
    if bench_enabled("inprocess_iddag_serde") {
        inprocess_iddag_serde::main();
    }
    if bench_enabled("spanset") {
        spanset::main();
    }
    if bench_enabled("segment_sizes") {
        segment_sizes::main();
    }
}
