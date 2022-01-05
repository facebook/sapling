/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use parking_lot::Mutex;
use regex::Regex;
use types::Key;
use types::RepoPathBuf;

use crate::StoreKey;

// TODO(meyer): This was implemented for ovrsource migration, and shouldn't be needed anymore.
pub struct FetchLogger {
    filter: Option<Regex>,
    seen: Mutex<HashSet<RepoPathBuf>>,
}

impl FetchLogger {
    pub fn new(filter: Option<Regex>) -> Self {
        Self {
            filter,
            seen: Mutex::new(HashSet::new()),
        }
    }

    pub fn take_seen(&self) -> HashSet<RepoPathBuf> {
        let mut seen = self.seen.lock();
        std::mem::take(&mut *seen)
    }

    pub fn report_store_keys<'a>(&self, keys: impl Iterator<Item = &'a StoreKey>) {
        self.report_keys(keys.filter_map(|k| k.maybe_as_key()))
    }

    pub fn report_keys<'a>(&self, keys: impl Iterator<Item = &'a Key>) {
        if let Some(filter) = &self.filter {
            let mut matches = Vec::new();
            for path in keys
                .map(|k| &k.path)
                .filter(|p| filter.is_match(p.as_str()))
            {
                matches.push(path.clone());
            }

            if !matches.is_empty() {
                let mut seen = self.seen.lock();
                seen.extend(matches.into_iter());
            }
        }
    }
}
