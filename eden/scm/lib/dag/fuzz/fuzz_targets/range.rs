/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![no_main]

use std::sync::LazyLock;

use bindag::TestContext;
use libfuzzer_sys::fuzz_target;

mod tests;

// The complete DAG is too large for `range` operation to run reasonably fast.
// Therefore take a subset of it.
static CONTEXT: LazyLock<TestContext> =
    LazyLock::new(|| TestContext::from_bin_sliced(bindag::MOZILLA, 49040..60415));

fuzz_target!(|input: (Vec<u16>, Vec<u16>)| {
    let roots = CONTEXT.clamp_revs(&input.0);
    let heads = CONTEXT.clamp_revs(&input.1);
    tests::test_range(&CONTEXT, roots, heads);
});
