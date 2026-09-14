/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use parking_lot::Mutex;

/// One repo's entry in [`crate::MononokeRepos`].
///
/// Slots are `Arc`-shared and outlive any individual snapshot of the repo map,
/// so a caller that resolved a slot from an older snapshot still observes state
/// transitions made through a newer one.
pub struct RepoSlot<R> {
    state: Mutex<SlotState<R>>,
}

enum SlotState<R> {
    /// Assigned to this host, but not built.
    Empty,
    Ready(Arc<R>),
}

impl<R> RepoSlot<R> {
    pub fn empty() -> Self {
        Self {
            state: Mutex::new(SlotState::Empty),
        }
    }

    pub fn ready(repo: Arc<R>) -> Self {
        Self {
            state: Mutex::new(SlotState::Ready(repo)),
        }
    }

    /// The built repo, or `None` if this slot has not been built.
    pub fn loaded(&self) -> Option<Arc<R>> {
        match &*self.state.lock() {
            SlotState::Empty => None,
            SlotState::Ready(repo) => Some(Arc::clone(repo)),
        }
    }
}
