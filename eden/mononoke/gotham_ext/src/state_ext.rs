/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use gotham::state::request_id;
use gotham::state::State;

pub trait StateExt {
    fn short_request_id(&self) -> &str;
}

const SHORT_ID_LEN: usize = 5;

impl StateExt for State {
    fn short_request_id(&self) -> &str {
        let req = request_id(self);
        &req[0..SHORT_ID_LEN.min(req.len())]
    }
}
