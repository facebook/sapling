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
const USE_ONLY_OFFSET_DELTA: &str = "x-git-only-offset-delta";
const PUSH_CONCURRENCY: &str = "x-git-push-concurrency";
const BYPASS_BOOKMARK_CACHE: &str = "x-git-bypass-bookmark-cache";

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
            .is_some_and(|v| **v == *b"1")
    }

    pub fn use_only_offset_delta(&self) -> bool {
        self.0
            .get(USE_ONLY_OFFSET_DELTA)
            .is_some_and(|v| **v == *b"1")
    }

    pub fn bypass_bookmark_cache(&self) -> bool {
        self.0
            .get(BYPASS_BOOKMARK_CACHE)
            .is_some_and(|v| **v == *b"1")
    }

    pub fn concurrency(&self) -> usize {
        self.0
            .get(PUSH_CONCURRENCY)
            .and_then(|v| String::from_utf8_lossy(v).parse().ok())
            .unwrap_or(100)
            .clamp(10, 500)
    }
}

impl AsRef<HashMap<String, Bytes>> for Pushvars {
    fn as_ref(&self) -> &HashMap<String, Bytes> {
        &self.0
    }
}
