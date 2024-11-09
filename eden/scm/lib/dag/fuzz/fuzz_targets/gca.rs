/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![no_main]

use bindag::TestContext;
use lazy_static::lazy_static;
use libfuzzer_sys::fuzz_target;

mod tests;

lazy_static! {
    static ref CONTEXT: TestContext =
        TestContext::from_bin(bindag::MOZILLA).truncate(u16::max_value() as usize);
}

fuzz_target!(|input: Vec<u16>| {
    // gca with > 3 revs is less interesting to this test.
    let revs = CONTEXT.clamp_revs(&input[..input.len().min(3)]);
    tests::test_gca(&CONTEXT, revs);
});
