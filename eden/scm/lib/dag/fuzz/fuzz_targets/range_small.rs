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

static CONTEXT: LazyLock<TestContext> = LazyLock::new(|| {
    let start = 60146;
    TestContext::from_bin_sliced(bindag::MOZILLA, start..start + 256)
});

fuzz_target!(|input: (Vec<u8>, Vec<u8>)| {
    let roots = CONTEXT.clamp_revs(&input.0);
    let heads = CONTEXT.clamp_revs(&input.1);
    tests::test_range(&CONTEXT, roots, heads);
});
