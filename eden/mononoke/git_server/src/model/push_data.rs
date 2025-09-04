/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham_derive::StateData;

/// Struct to hold the data for a git push request. Can include arbitrary
/// request level properties.
#[derive(Clone, StateData)]
pub struct PushData {
    pub packfile_size: usize,
}

impl PushData {
    pub fn inject_in_state(state: &mut gotham::state::State, packfile_size: usize) {
        state.put(PushData { packfile_size });
    }
}
