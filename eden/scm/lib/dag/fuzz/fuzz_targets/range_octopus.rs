/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![no_main]

use bindag::octopus;
use bindag::OctopusTestContext;
use lazy_static::lazy_static;
use libfuzzer_sys::fuzz_target;

mod tests;

lazy_static! {
    static ref CONTEXT: OctopusTestContext =
        OctopusTestContext::from_parents(octopus::cross_octopus());
}

fuzz_target!(|input: (Vec<u8>, Vec<u8>)| {
    let roots = CONTEXT.clamp_revs(&input.0);
    let heads = CONTEXT.clamp_revs(&input.1);
    tests::test_range(&CONTEXT, roots, heads);
});
