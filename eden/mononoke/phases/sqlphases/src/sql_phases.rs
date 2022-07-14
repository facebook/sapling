/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;

use abomonation_derive::Abomonation;
use anyhow::Error;
use anyhow::Result;
use ascii::AsciiString;
use async_trait::async_trait;
use changeset_fetcher::ArcChangesetFetcher;
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use futures::future::try_join;
use futures::future::BoxFuture;
use futures::future::FutureExt;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use phases::ArcPhases;
use phases::Phase;
use phases::Phases;
use sql::mysql;
use sql::mysql_async::prelude::ConvIr;
use sql::mysql_async::prelude::FromValue;
use sql::mysql_async::FromValueError;
use sql::mysql_async::Value;

use crate::errors::SqlPhasesError;
use crate::sql_store::SqlPhasesStore;

/// Newtype wrapper for Phase that allows us to derive SQL conversions.
#[derive(Abomonation, Clone, Copy, PartialEq, Eq, Debug)]
#[derive(mysql::OptTryFromRowField)]
#[repr(transparent)]
pub struct SqlPhase(pub Phase);

impl From<SqlPhase> for Phase {
    fn from(phase: SqlPhase) -> Phase {
        phase.0
    }
}

impl From<Phase> for SqlPhase {
    fn from(phase: Phase) -> SqlPhase {
        SqlPhase(phase)
    }
}

impl fmt::Display for SqlPhase {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl TryFrom<&[u8]> for SqlPhase {
    type Error = SqlPhasesError;

    fn try_from(buf: &[u8]) -> Result<Self, Self::Error> {
        match std::str::from_utf8(buf) {
            Ok("Draft") => Ok(SqlPhase(Phase::Draft)),
            Ok("Public") => Ok(SqlPhase(Phase::Public)),
            Ok(s) => Err(SqlPhasesError::ValueError(s.to_string())),
            _ => Err(SqlPhasesError::ParseError(buf.len())),
        }
    }
}

impl From<SqlPhase> for Value {
    fn from(phase: SqlPhase) -> Self {
        Value::Bytes(phase.to_string().into())
    }
}

impl FromValue for SqlPhase {
    type Intermediate = SqlPhase;
}

impl ConvIr<SqlPhase> for SqlPhase {
    fn new(v: Value) -> Result<Self, FromValueError> {
        match v {
            Value::Bytes(bytes) => AsciiString::from_ascii(bytes)
                .map_err(|err| FromValueError(Value::Bytes(err.into_source())))
                .and_then(|s| match s.as_str() {
                    "Draft" => Ok(SqlPhase(Phase::Draft)),
                    "Public" => Ok(SqlPhase(Phase::Public)),
                    _ => Err(FromValueError(Value::Bytes(s.into()))),
                }),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> SqlPhase {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

pub type HeadsFetcher =
    Arc<dyn Fn(&CoreContext) -> BoxFuture<'static, Result<Vec<ChangesetId>, Error>> + Send + Sync>;

#[derive(Clone)]
pub struct SqlPhases {
    phases_store: SqlPhasesStore,
    changeset_fetcher: ArcChangesetFetcher,
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
        let public_cold = self.get_public_raw(ctx, &csids).await?;

        let mut unknown: Vec<_> = csids
            .into_iter()
            .filter(|csid| !public_cold.contains(csid))
            .collect();

        if unknown.is_empty() {
            return Ok(public_cold);
        }

        let heads = (self.heads_fetcher)(ctx).await?;
        let freshly_marked = mark_reachable_as_public(ctx, self, &heads, ephemeral_derive).await?;

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
        changeset_fetcher: ArcChangesetFetcher,
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
    let public = phases.get_public_raw(ctx, all_heads).await?;

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

        let phase = phases.get_single_raw(ctx, cs).await?;
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
            phases.add_public_raw(ctx, chunk.clone()).await?;
        }
        result.extend(chunk)
    }
    Ok(result)
}
