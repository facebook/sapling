/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod errors;

use std::collections::HashSet;
use std::fmt;

use abomonation_derive::Abomonation;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
pub use errors::PhasesError;
use mononoke_types::ChangesetId;

#[derive(Abomonation, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Draft,
    Public,
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Phase::Draft => write!(f, "Draft"),
            Phase::Public => write!(f, "Public"),
        }
    }
}

impl From<Phase> for u32 {
    fn from(phase: Phase) -> u32 {
        match phase {
            Phase::Public => 0,
            Phase::Draft => 1,
        }
    }
}

impl TryFrom<u32> for Phase {
    type Error = PhasesError;

    fn try_from(phase_as_int: u32) -> Result<Phase, Self::Error> {
        match phase_as_int {
            0 => Ok(Phase::Public),
            1 => Ok(Phase::Draft),
            _ => Err(PhasesError::EnumError(phase_as_int)),
        }
    }
}

/// Phases tracks which commits are public, and which commits are draft.
///
/// A commit ordinarily becomes public when it is reachable from any
/// publishing bookmark.  Once public, it never becomes draft again, even
/// if the public bookmark is deleted or moved elsewhere.
#[facet::facet]
#[async_trait]
pub trait Phases: Send + Sync {
    /// Mark all commits reachable from heads as public.  Returns all
    /// the newly public commits.
    async fn add_reachable_as_public(
        &self,
        ctx: &CoreContext,
        heads: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetId>>;

    /// Add the given commits as public.  The caller is responsible
    /// for ensuring that the ancestors of all of these commits are
    /// already public, and the commits are provided in topological
    /// order.
    async fn add_public_with_known_public_ancestors(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<()>;

    /// Returns the commits that are public.  This method will attempt
    /// to check if any of these commits have recently become public.
    async fn get_public(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
        ephemeral_derive: bool,
    ) -> Result<HashSet<ChangesetId>>;

    /// Returns the commits that are known to be public in the cache.
    /// Commits that have recently become public might not be included,
    /// however this method is more performant than `get_public`.
    async fn get_cached_public(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<HashSet<ChangesetId>>;

    /// List all public commits.
    async fn list_all_public(&self, ctx: &CoreContext) -> Result<Vec<ChangesetId>>;

    /// Return a copy of this phases object with the set of public
    /// heads frozen.
    fn with_frozen_public_heads(&self, heads: Vec<ChangesetId>) -> ArcPhases;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_as_integer() {
        assert_eq!(u32::from(Phase::Public), 0);
        assert_eq!(u32::from(Phase::Draft), 1);
        assert_eq!(Phase::try_from(0u32), Ok(Phase::Public));
        assert_eq!(Phase::try_from(1u32), Ok(Phase::Draft));
        assert!(Phase::try_from(2u32).is_err());
    }
}
