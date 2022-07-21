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

fn main() {
    dag_ops::main();
    inprocess_iddag_serde::main();
    spanset::main();
    segment_sizes::main();
}
