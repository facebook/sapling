/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::{FromState, State};

use crate::context::EdenApiServerContext;

pub fn health_handler(state: State) -> (State, &'static str) {
    if EdenApiServerContext::borrow_from(&state).will_exit() {
        (state, "EXITING")
    } else {
        (state, "I_AM_ALIVE")
    }
}
