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
    static ref CONTEXT: TestContext =
        TestContext::from_bin(bindag::GIT).truncate(u16::max_value() as usize);
}

fuzz_target!(|input: Vec<u16>| {
    // gca with > 3 revs is less interesting to this test.
    let revs = CONTEXT.clamp_revs(&input[..input.len().min(3)]);
    tests::test_gca(&CONTEXT, revs);
});
