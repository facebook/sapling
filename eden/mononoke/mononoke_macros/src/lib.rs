/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod mononoke {
    use justknobs::test_helpers;
    use justknobs::test_helpers::JustKnobsInMemory;
    pub use mononoke_proc_macros::fbinit_test;
    pub use mononoke_proc_macros::test;

    pub fn override_just_knobs() {
        let just_knobs_json = include_str!("../just_knobs_defaults/just_knobs.json");
        test_helpers::override_just_knobs(
            JustKnobsInMemory::from_json(just_knobs_json).expect("failed to parse just knobs json"),
        );
    }
}
