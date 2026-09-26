/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use arc_swap::ArcSwap;
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::Shared;
use parking_lot::Mutex;

/// The result of one build, handed to every caller waiting on it.
///
/// The error is `Arc`-wrapped because `anyhow::Error` is not `Clone`.
type BuildOutcome<R> = Result<Arc<R>, Arc<Error>>;

/// A handle on an in-flight build, shared by every caller waiting on it.
type SharedBuild<R> = Shared<oneshot::Receiver<BuildOutcome<R>>>;

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
    name: String,
    /// Read on every repo lookup, so it is swapped rather than locked: serving
    /// a built repo is a read, and readers must not have to exclude each other
    /// to do it.
    state: ArcSwap<SlotState<R>>,
    /// Serializes writers: deciding who starts a build is a read-decide-write,
    /// which no atomic load expresses. Readers never take it.
    transition: Mutex<()>,
}

enum SlotState<R> {
    /// Assigned to this host, but not built.
    Empty,
    /// A build is in flight; this is the handle it reports through.
    Building(SharedBuild<R>),
    /// Built, and available to serve.
    Ready(Arc<R>),
}

/// What [`RepoSlot::claim_build`] decided the caller should do.
pub enum Claim<R> {
    /// Already built.
    Ready(Arc<R>),
    /// Someone else is already building this repo.
    Wait(SharedBuild<R>),
    /// This caller won the race: run the build, report it through the
    /// completion handle, then wait on the shared handle like everyone else.
    Start(BuildCompletion<R>, SharedBuild<R>),
}

impl<R> RepoSlot<R> {
    /// A slot for a repo assigned to this service but not built.
    pub(crate) fn empty(name: String) -> Self {
        Self::in_state(name, SlotState::Empty)
    }

    /// A slot for a repo that is already built.
    pub(crate) fn ready(name: String, repo: Arc<R>) -> Self {
        Self::in_state(name, SlotState::Ready(repo))
    }

    fn in_state(name: String, state: SlotState<R>) -> Self {
        Self {
            name,
            state: ArcSwap::from_pointee(state),
            transition: Mutex::new(()),
        }
    }

    /// Which repo this slot is for. A slot outlives any one map snapshot, so it
    /// has to carry its own identity rather than rely on the key it was found
    /// under.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The built repo, or `None` if this slot has not been built.
    pub fn loaded(&self) -> Option<Arc<R>> {
        match &**self.state.load() {
            SlotState::Empty | SlotState::Building(_) => None,
            SlotState::Ready(repo) => Some(Arc::clone(repo)),
        }
    }

    /// The claim this slot can answer without taking the lock. `None` when it
    /// is `Empty`, the one case that needs a writer.
    fn peek(&self) -> Option<Claim<R>> {
        match &**self.state.load() {
            SlotState::Ready(repo) => Some(Claim::Ready(Arc::clone(repo))),
            SlotState::Building(pending) => Some(Claim::Wait(pending.clone())),
            SlotState::Empty => None,
        }
    }

    /// Resolves the slot to the built repo, an in-flight build to wait on, or
    /// the obligation to run one. The build is not run here and needs no
    /// runtime: this decides *who* builds, not *how*.
    ///
    /// Exactly one caller is handed [`Claim::Start`] per build, and dropping
    /// its [`BuildCompletion`] without finishing publishes a failure, so a
    /// claim can never be silently abandoned.
    ///
    /// Single-flight holds **per slot, not per repo name**: `remove` followed
    /// by a re-`add` of the same name makes a *new* slot, so a build against
    /// the old one can still be in flight. Harmless while nothing evicts - the
    /// orphaned slot is unreachable and its result dropped - but eviction has
    /// to account for it.
    pub fn claim_build(self: &Arc<Self>) -> Claim<R> {
        if let Some(claim) = self.peek() {
            return claim;
        }

        let transition = self.transition.lock();

        // Another writer may have started or finished a build since the peek
        // above.
        if let Some(claim) = self.peek() {
            return claim;
        }

        let (send_outcome, outcome) = oneshot::channel();
        let pending = outcome.shared();
        self.state
            .store(Arc::new(SlotState::Building(pending.clone())));
        drop(transition);

        Claim::Start(
            BuildCompletion {
                repo_slot: Arc::clone(self),
                send_outcome: Some(send_outcome),
            },
            pending,
        )
    }

    /// Publishes a finished build into this slot.
    ///
    /// Deliberately has no "was this repo removed?" check: this writes only
    /// slot interior, never the repo map, so a slot that `remove` or a
    /// replacing `add` detached is unreachable whatever is written to it. Keep
    /// it that way - writing back to the map from here would reintroduce the
    /// race `reload_if_present` exists to close.
    fn apply_outcome(&self, outcome: &BuildOutcome<R>) {
        let next = match outcome {
            Ok(repo) => SlotState::Ready(Arc::clone(repo)),
            // Back to `Empty` so the next caller retries rather than inheriting
            // this failure.
            Err(_) => SlotState::Empty,
        };
        let _transition = self.transition.lock();
        self.state.store(Arc::new(next));
    }
}

/// The obligation that comes with winning a [`Claim::Start`]: publish the
/// build's result into the slot and hand it to every waiter.
///
/// Dropping it without calling [`BuildCompletion::finish`] publishes a failure
/// instead, so a build that panics, or is dropped by a shutting-down runtime,
/// cannot leave the slot stuck in [`SlotState::Building`] behind an
/// already-resolved handle - a state nothing recovers from, because the slot
/// never looks claimable again.
pub struct BuildCompletion<R> {
    repo_slot: Arc<RepoSlot<R>>,
    send_outcome: Option<oneshot::Sender<BuildOutcome<R>>>,
}

impl<R> BuildCompletion<R> {
    /// Publishes the result of the build. Takes `self` by value so the
    /// obligation is discharged exactly once.
    pub fn finish(mut self, outcome: BuildOutcome<R>) {
        self.publish(outcome);
    }

    /// Applies the outcome before sending, so a waiter woken by it always
    /// observes the slot in its final state rather than still `Building`.
    fn publish(&mut self, outcome: BuildOutcome<R>) {
        let Some(send_outcome) = self.send_outcome.take() else {
            return;
        };
        self.repo_slot.apply_outcome(&outcome);
        let _ = send_outcome.send(outcome);
    }
}

impl<R> Drop for BuildCompletion<R> {
    fn drop(&mut self) {
        // `publish` would no-op anyway once `finish` has taken the sender. The
        // check is to not build an error message on every successful build.
        if self.send_outcome.is_none() {
            return;
        }
        let repo_name = self.repo_slot.name().to_string();
        self.publish(Err(Arc::new(anyhow!(
            "Build of repo {repo_name} ended without producing a result"
        ))));
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_empty_slot_holds_no_repo() {
        let repo_slot: RepoSlot<i32> = RepoSlot::empty("foo".to_string());
        assert!(
            repo_slot.loaded().is_none(),
            "an unbuilt slot must look absent to every reader"
        );
    }

    #[mononoke::test]
    fn test_ready_slot_hands_out_its_repo() {
        let repo_slot = RepoSlot::ready("foo".to_string(), Arc::new(42));
        assert_eq!(repo_slot.loaded().as_deref(), Some(&42));
    }

    #[mononoke::test]
    fn test_reading_a_slot_shares_rather_than_copies() {
        let repo = Arc::new(42);
        let repo_slot = RepoSlot::ready("foo".to_string(), Arc::clone(&repo));

        // Two reads must hand back the same allocation, not clones of the repo:
        // a repo is expensive and callers rely on sharing one instance.
        let first = repo_slot.loaded().expect("slot was built");
        let second = repo_slot.loaded().expect("slot was built");
        assert!(Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(&first, &repo));
    }
}
