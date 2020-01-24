/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![no_main]

use bindag::{octopus, OctopusTestContext};
use lazy_static::lazy_static;
use libfuzzer_sys::fuzz_target;

mod tests;

lazy_static! {
    static ref CONTEXT: OctopusTestContext =
        OctopusTestContext::from_parents(octopus::cross_octopus());
}

fuzz_target!(|input: Vec<u8>| {
    let revs = CONTEXT.clamp_revs(&input[..input.len().min(5)]);
    tests::test_gca(&CONTEXT, revs);
});
