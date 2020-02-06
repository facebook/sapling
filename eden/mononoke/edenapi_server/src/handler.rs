/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use gotham::{router::builder::*, state::State};
use gotham_ext::handler::MononokeHttpHandler;

pub fn build_handler() -> MononokeHttpHandler {
    let router = build_simple_router(|route| {
        route.get("health_check").to(health_check);
    });
    MononokeHttpHandler::builder().build(router)
}

fn health_check(state: State) -> (State, &'static str) {
    (state, "I_AM_ALIVE")
}
