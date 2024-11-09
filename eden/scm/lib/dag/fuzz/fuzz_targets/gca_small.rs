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
    static ref CONTEXT: TestContext = {
        let start = 60146;
        TestContext::from_bin_sliced(bindag::MOZILLA, start..start + 256)
    };
}

fuzz_target!(|input: Vec<u8>| {
    // plain gca supports 6 revs at maximum.
    let revs = CONTEXT.clamp_revs(&input[..input.len().min(6)]);
    tests::test_gca(&CONTEXT, revs);
});
