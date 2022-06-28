/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ahash::RandomState;
use anyhow::bail;
use anyhow::Error;
use dashmap::DashMap;

#[derive(Debug)]
struct TrackData {
    pos: u64,
    done: bool,
}

// Track whether a key is done or pending
pub struct Tracker {
    state: DashMap<String, TrackData, RandomState>,
}

impl Tracker {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            state: DashMap::with_capacity_and_hasher(capacity, RandomState::default()),
        }
    }

    // Start tracking a key
    pub fn insert(&self, key: String, pos: u64) {
        self.state.insert(key, TrackData { pos, done: false });
    }

    pub fn mark_done(&self, key: &str) -> Result<(), Error> {
        if let Some(mut tracked) = self.state.get_mut(key) {
            tracked.done = true;
        } else {
            bail!("No inflight entry for {}, may have duplicates", key);
        };
        Ok(())
    }

    // Finds latest done key, if any, before the earliest pending key, and prunes the state before the done key.
    pub fn compact(&self) -> Option<String> {
        let mut earliest_pending = None;
        for i in self.state.iter() {
            let tracked = i.value();
            if !tracked.done {
                let replace = if let Some(ref best) = earliest_pending {
                    tracked.pos < *best
                } else {
                    true
                };
                if replace {
                    earliest_pending.replace(tracked.pos);
                }
            }
        }
        let mut best_done = None;
        for i in self.state.iter() {
            let tracked = i.value();
            let in_bound = if let Some(bound) = earliest_pending.as_ref() {
                tracked.pos < *bound
            } else {
                true
            };
            if tracked.done && in_bound {
                let replace = if let Some((_, best)) = best_done.as_ref() {
                    tracked.pos > *best
                } else {
                    true
                };
                if replace {
                    best_done.replace((i.key().clone(), tracked.pos));
                }
            }
        }

        // remove the used entries
        if let Some((_, done_pos)) = best_done.as_ref() {
            self.state.retain(|_k, v| v.pos > *done_pos);
        }
        best_done.map(|(key, _)| key)
    }
}
