/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use parking_lot::Mutex;

/// One repo's entry in [`crate::MononokeRepos`]: either the built repo, or
/// nothing yet.
///
/// # DO NOT BUILD ON THIS YET
///
/// This type is scaffolding for the in-progress inexpensive-repos work (lazy
/// repo loading) and **is not rolled out**. Nothing in production creates an
/// unbuilt slot, so today it is an implementation detail of the repo map and
/// not a supported way to model repo state.
///
/// If you are about to reach for `RepoSlot`, or for its unbuilt state, in new
/// code - **stop and talk to lmvasquezg first**. This applies to coding agents
/// as much as to people: the rollout has not settled what an unbuilt repo means
/// for callers, and depending on it early bakes in answers that are still being
/// decided.
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
    /// Built, and available to serve.
    Ready(Arc<R>),
}

impl<R> RepoSlot<R> {
    /// A slot for a repo assigned to this service but not built.
    pub fn empty() -> Self {
        Self {
            state: Mutex::new(SlotState::Empty),
        }
    }

    /// A slot for a repo that is already built.
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

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_empty_slot_holds_no_repo() {
        let slot: RepoSlot<i32> = RepoSlot::empty();
        assert!(
            slot.loaded().is_none(),
            "an unbuilt slot must look absent to every reader"
        );
    }

    #[mononoke::test]
    fn test_ready_slot_hands_out_its_repo() {
        let slot = RepoSlot::ready(Arc::new(42));
        assert_eq!(slot.loaded().as_deref(), Some(&42));
    }

    #[mononoke::test]
    fn test_reading_a_slot_shares_rather_than_copies() {
        let repo = Arc::new(42);
        let slot = RepoSlot::ready(Arc::clone(&repo));

        // Two reads must hand back the same allocation, not clones of the repo:
        // a repo is expensive and callers rely on sharing one instance.
        let first = slot.loaded().expect("slot was built");
        let second = slot.loaded().expect("slot was built");
        assert!(Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&first, &repo));
    }
}
