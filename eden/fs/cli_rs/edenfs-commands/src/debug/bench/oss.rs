/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Open-source builds have no LMDB crate, so its benchmarks are skipped.

use anyhow::Result;

use super::r#gen::RandomData;
use super::r#gen::TestDir;

pub fn run_lmdb_benchmarks(_test_dir: &TestDir, _random_data: &RandomData) -> Result<()> {
    println!("LMDB benchmarks are not available in this build; skipping to SQLite.");
    Ok(())
}
