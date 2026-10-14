/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![no_main]

use std::sync::LazyLock;

use bindag::TestContext;

mod tests;

static CONTEXT: LazyLock<TestContext> =
    LazyLock::new(|| TestContext::from_bin(bindag::MOZILLA).truncate(u16::MAX as usize));

tests::fuzz_target_with_context!(|input: Vec<u16>| {
    // gca with > 3 revs is less interesting to this test.
    let revs = CONTEXT.clamp_revs(&input[..input.len().min(3)]);
    tests::test_gca(&CONTEXT, revs);
});
