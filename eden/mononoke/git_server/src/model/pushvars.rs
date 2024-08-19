/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use bytes::Bytes;
use gotham_derive::StateData;

const WAIT_FOR_WBC_UPDATE: &str = "x-git-read-after-write-consistency";

#[derive(Clone, StateData)]
pub struct Pushvars(HashMap<String, Bytes>);

impl Pushvars {
    pub fn new(pushvars: HashMap<String, Bytes>) -> Self {
        Self(pushvars)
    }

    pub fn wait_for_wbc_update(&self) -> bool {
        self.0
            .get(WAIT_FOR_WBC_UPDATE)
            .map_or(false, |v| **v == *b"1")
    }
}

impl AsRef<HashMap<String, Bytes>> for Pushvars {
    fn as_ref(&self) -> &HashMap<String, Bytes> {
        &self.0
    }
}
