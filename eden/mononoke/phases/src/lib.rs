/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod errors;
pub use errors::ErrorKind;
mod factory;
pub use factory::SqlPhasesBuilder;
mod sql_store;
pub use sql_store::SqlPhasesStore;

use abomonation_derive::Abomonation;
use anyhow::{Error, Result};
use ascii::AsciiString;
use async_trait::async_trait;
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use futures::future::{try_join, BoxFuture, FutureExt};
use mononoke_types::{ChangesetId, RepositoryId};
use sql::mysql;
use sql::mysql_async::{
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::Arc,
};

#[derive(Abomonation, Clone, Copy, PartialEq, Eq, Debug)]
#[derive(mysql::OptTryFromRowField)]
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

impl TryFrom<&[u8]> for Phase {
    type Error = ErrorKind;

    fn try_from(buf: &[u8]) -> Result<Self, Self::Error> {
        match std::str::from_utf8(&buf) {
            Ok("Draft") => Ok(Phase::Draft),
            Ok("Public") => Ok(Phase::Public),
            Ok(s) => Err(ErrorKind::PhasesError(format!(
                "Conversion error from &[u8] to Phase for {}",
                s
            ))),
            _ => Err(ErrorKind::PhasesError(format!(
                "Conversion error from &[u8] to Phase, received {} bytes",
                buf.len()
            ))),
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
    type Error = ErrorKind;

    fn try_from(phase_as_int: u32) -> Result<Phase, Self::Error> {
        match phase_as_int {
            0 => Ok(Phase::Public),
            1 => Ok(Phase::Draft),
            _ => Err(ErrorKind::PhasesError(format!(
                "Cannot convert integer {} to a Phase",
                phase_as_int
            ))),
        }
    }
}

impl From<Phase> for Value {
    fn from(phase: Phase) -> Self {
        Value::Bytes(phase.to_string().into())
    }
}

impl FromValue for Phase {
    type Intermediate = Phase;
}

impl ConvIr<Phase> for Phase {
    fn new(v: Value) -> Result<Self, FromValueError> {
        match v {
            Value::Bytes(bytes) => AsciiString::from_ascii(bytes)
                .map_err(|err| FromValueError(Value::Bytes(err.into_source())))
                .and_then(|s| match s.as_str() {
                    "Draft" => Ok(Phase::Draft),
                    "Public" => Ok(Phase::Public),
                    _ => Err(FromValueError(Value::Bytes(s.into()))),
                }),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> Phase {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

/// This is the primary interface for clients to interact with Phases
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

pub type HeadsFetcher =
    Arc<dyn Fn(&CoreContext) -> BoxFuture<'static, Result<Vec<ChangesetId>, Error>> + Send + Sync>;

#[derive(Clone)]
pub struct SqlPhases {
    phases_store: SqlPhasesStore,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    heads_fetcher: HeadsFetcher,
    repo_id: RepositoryId,
}

impl SqlPhases {
    pub async fn get_single_raw(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<Phase>, Error> {
        self.phases_store
            .get_single_raw(ctx, self.repo_id, cs_id)
            .await
    }

    pub async fn get_public_raw(
        &self,
        ctx: &CoreContext,
        csids: impl IntoIterator<Item = &ChangesetId>,
    ) -> Result<HashSet<ChangesetId>, Error> {
        self.phases_store
            .get_public_raw(ctx, self.repo_id, csids)
            .await
    }

    pub async fn add_public_raw(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<(), Error> {
        self.phases_store
            .add_public_raw(ctx, self.repo_id, csids)
            .await
    }

    pub async fn list_all_public(&self, ctx: &CoreContext) -> Result<Vec<ChangesetId>, Error> {
        self.phases_store.list_all_public(ctx, self.repo_id).await
    }

    pub async fn get_public_derive(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
        ephemeral_derive: bool,
    ) -> Result<HashSet<ChangesetId>, Error> {
        if csids.is_empty() {
            return Ok(Default::default());
        }
        let public_cold = self.get_public_raw(&ctx, &csids).await?;

        let mut unknown: Vec<_> = csids
            .into_iter()
            .filter(|csid| !public_cold.contains(csid))
            .collect();

        if unknown.is_empty() {
            return Ok(public_cold);
        }

        let heads = (self.heads_fetcher)(&ctx).await?;
        let freshly_marked = mark_reachable_as_public(ctx, &self, &heads, ephemeral_derive).await?;

        // Still do the get_public_raw incase someone else marked the changes as public
        // and thus mark_reachable_as_public did not return them as freshly_marked
        let public_hot = self.get_public_raw(ctx, &unknown).await?;

        let public_combined = public_cold.into_iter().chain(public_hot);
        let public_combined = if ephemeral_derive {
            unknown.retain(|e| freshly_marked.contains(e));
            public_combined.chain(unknown).collect()
        } else {
            public_combined.collect()
        };

        Ok(public_combined)
    }
}

impl SqlPhases {
    pub fn new(
        phases_store: SqlPhasesStore,
        repo_id: RepositoryId,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
        heads_fetcher: HeadsFetcher,
    ) -> Self {
        Self {
            phases_store,
            changeset_fetcher,
            heads_fetcher,
            repo_id,
        }
    }
}

#[async_trait]
impl Phases for SqlPhases {
    async fn get_public(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
        ephemeral_derive: bool,
    ) -> Result<HashSet<ChangesetId>> {
        self.get_public_derive(ctx, csids, ephemeral_derive).await
    }

    async fn get_cached_public(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<HashSet<ChangesetId>> {
        self.get_public_raw(ctx, &csids).await
    }

    async fn list_all_public(&self, ctx: &CoreContext) -> Result<Vec<ChangesetId>> {
        self.list_all_public(ctx).await
    }

    async fn add_reachable_as_public(
        &self,
        ctx: &CoreContext,
        heads: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetId>> {
        mark_reachable_as_public(ctx, self, &heads, false).await
    }

    async fn add_public_with_known_public_ancestors(
        &self,
        ctx: &CoreContext,
        csids: Vec<ChangesetId>,
    ) -> Result<()> {
        self.add_public_raw(ctx, csids).await
    }

    fn with_frozen_public_heads(&self, heads: Vec<ChangesetId>) -> ArcPhases {
        let heads_fetcher = Arc::new(move |_ctx: &CoreContext| {
            let heads = heads.clone();
            async move { Ok(heads) }.boxed()
        });
        Arc::new(SqlPhases {
            phases_store: self.phases_store.clone(),
            changeset_fetcher: self.changeset_fetcher.clone(),
            heads_fetcher,
            repo_id: self.repo_id,
        })
    }
}

/// Mark all commits reachable from `public_heads` as public
pub async fn mark_reachable_as_public(
    ctx: &CoreContext,
    phases: &SqlPhases,
    all_heads: &[ChangesetId],
    ephemeral_derive: bool,
) -> Result<Vec<ChangesetId>, Error> {
    let changeset_fetcher = &phases.changeset_fetcher;
    let public = phases.get_public_raw(&ctx, all_heads).await?;

    let mut input = all_heads
        .iter()
        .filter(|csid| !public.contains(csid))
        .copied()
        .collect::<Vec<_>>();

    let mut unmarked = HashMap::new();
    loop {
        let cs = match input.pop() {
            None => {
                break;
            }
            Some(cs) => cs,
        };

        let phase = phases.get_single_raw(&ctx, cs).await?;
        if let Some(Phase::Public) = phase {
            continue;
        }

        let (generation, parents) = try_join(
            changeset_fetcher.get_generation_number(ctx.clone(), cs),
            changeset_fetcher.get_parents(ctx.clone(), cs),
        )
        .await?;

        unmarked.insert(cs, generation);
        input.extend(parents.into_iter().filter(|p| !unmarked.contains_key(p)));
    }

    // NOTE: We need to write phases in increasing generation number order, this will
    //       ensure that our phases in a valid state (i.e do not have any gaps). Once
    //       first public changeset is found we assume that all ancestors of it have
    //       already been marked as public.
    let mut unmarked: Vec<_> = unmarked.into_iter().collect();
    unmarked.sort_by(|l, r| l.1.cmp(&r.1));

    let mut result = vec![];
    for chunk in unmarked.chunks(100) {
        let chunk = chunk.iter().map(|(cs, _)| *cs).collect::<Vec<_>>();
        if !ephemeral_derive {
            phases.add_public_raw(&ctx, chunk.clone()).await?;
        }
        result.extend(chunk)
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_phase_as_integer() {
        assert_eq!(u32::from(Phase::Public), 0);
        assert_eq!(u32::from(Phase::Draft), 1);
        assert_eq!(Phase::try_from(u32::from(Phase::Public)), Ok(Phase::Public));
        assert_eq!(Phase::try_from(u32::from(Phase::Draft)), Ok(Phase::Draft));
    }
}
