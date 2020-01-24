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
    // The complete DAG is too large for `range` operation to run reasonably fast.
    // Therefore take a subset of it.
    static ref CONTEXT: TestContext = TestContext::from_bin_sliced(bindag::GIT, 49040..60415);
}

fuzz_target!(|input: (Vec<u16>, Vec<u16>)| {
    let roots = CONTEXT.clamp_revs(&input.0);
    let heads = CONTEXT.clamp_revs(&input.1);
    tests::test_range(&CONTEXT, roots, heads);
});
