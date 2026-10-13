/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![no_main]

use std::sync::LazyLock;

use bindag::OctopusTestContext;
use bindag::octopus;

mod tests;

static CONTEXT: LazyLock<OctopusTestContext> =
    LazyLock::new(|| OctopusTestContext::from_parents(octopus::cross_octopus()));

tests::fuzz_target_with_context!(|input: (Vec<u8>, Vec<u8>)| {
    let roots = CONTEXT.clamp_revs(&input.0);
    let heads = CONTEXT.clamp_revs(&input.1);
    tests::test_range(&CONTEXT, roots, heads);
});
