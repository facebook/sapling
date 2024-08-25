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
const METAGIT_BYPASS_ALL_HOOKS: &str = "x-metagit-bypass-hooks";

#[derive(Clone, StateData)]
pub struct Pushvars(HashMap<String, Bytes>);

impl Pushvars {
    pub fn new(pushvars: HashMap<String, Bytes>) -> Self {
        let pushvars = pushvars
            .into_iter()
            .map(|(name, value)| {
                if name.as_str() == METAGIT_BYPASS_ALL_HOOKS {
                    // Mononoke doesn't understand Metagit bypass pushvar, so update it accordingly
                    ("BYPASS_ALL_HOOKS".to_string(), value)
                } else {
                    (name, value)
                }
            })
            .collect();
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
