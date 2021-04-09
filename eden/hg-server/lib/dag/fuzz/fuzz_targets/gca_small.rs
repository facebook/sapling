/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![no_main]

use bindag::TestContext;
use lazy_static::lazy_static;
use libfuzzer_sys::fuzz_target;

mod tests;

lazy_static! {
    static ref CONTEXT: TestContext = {
        let start = 60146;
        TestContext::from_bin_sliced(bindag::GIT, start..start + 256)
    };
}

fuzz_target!(|input: Vec<u8>| {
    // plain gca supports 6 revs at maximum.
    let revs = CONTEXT.clamp_revs(&input[..input.len().min(6)]);
    tests::test_gca(&CONTEXT, revs);
});
