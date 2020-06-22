/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod caching;
mod errors;
pub use errors::ErrorKind;
mod factory;
pub use factory::SqlPhasesFactory;
mod sql_store;
pub use sql_store::SqlPhasesStore;

use abomonation_derive::Abomonation;
use anyhow::{Error, Result};
use ascii::AsciiString;
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use futures::{
    compat::Future01CompatExt,
    future::{try_join, BoxFuture as NewBoxFuture, FutureExt},
    TryFutureExt,
};
use futures_ext::{BoxFuture, FutureExt as OldFutureExt};
use mononoke_types::{ChangesetId, RepositoryId};
use sql::mysql_async::{
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};
use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    fmt,
    sync::Arc,
};

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
pub trait Phases: Send + Sync {
    /// mark all commits reachable from heads as public
    fn add_reachable_as_public(
        &self,
        ctx: CoreContext,
        heads: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetId>, Error>;

    fn get_public(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
        ephemeral_derive: bool,
    ) -> BoxFuture<HashSet<ChangesetId>, Error>;

    fn get_sql_phases(&self) -> &SqlPhases;
}

pub type HeadsFetcher = Arc<
    dyn Fn(&CoreContext) -> NewBoxFuture<'static, Result<Vec<ChangesetId>, Error>> + Send + Sync,
>;

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
        csids: &[ChangesetId],
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

    pub async fn list_all_public(&self, ctx: CoreContext) -> Result<Vec<ChangesetId>, Error> {
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

impl Phases for SqlPhases {
    fn get_public(
        &self,
        ctx: CoreContext,
        csids: Vec<ChangesetId>,
        ephemeral_derive: bool,
    ) -> BoxFuture<HashSet<ChangesetId>, Error> {
        let this = self.clone();
        async move { this.get_public_derive(&ctx, csids, ephemeral_derive).await }
            .boxed()
            .compat()
            .boxify()
    }

    fn add_reachable_as_public(
        &self,
        ctx: CoreContext,
        heads: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetId>, Error> {
        let this = self.clone();
        async move { mark_reachable_as_public(&ctx, &this, &heads, false).await }
            .boxed()
            .compat()
            .boxify()
    }

    fn get_sql_phases(&self) -> &SqlPhases {
        self
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
    let public = phases.get_public_raw(&ctx, &all_heads).await?;

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
            changeset_fetcher
                .get_generation_number(ctx.clone(), cs)
                .compat(),
            changeset_fetcher.get_parents(ctx.clone(), cs).compat(),
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
