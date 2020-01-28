/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

mod caching;
pub use caching::CachingPhases;
mod errors;
pub use errors::ErrorKind;

use anyhow::{Error, Result};
use ascii::AsciiString;
use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use futures::{
    future::{self, IntoFuture, Loop},
    stream, Future, Stream,
};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::HgPhase;
use mononoke_types::{ChangesetId, RepositoryId};
use sql::mysql_async::{
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};
use sql::{queries, Connection};
pub use sql_ext::SqlConstructors;
use stats::prelude::*;
use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    fmt,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Draft,
    Public,
}

impl From<Phase> for HgPhase {
    fn from(phase: Phase) -> HgPhase {
        match phase {
            Phase::Public => HgPhase::Public,
            Phase::Draft => HgPhase::Draft,
        }
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Phase::Draft => write!(f, "Draft"),
            Phase::Public => write!(f, "Public"),
        }
    }
}

impl TryFrom<iobuf::IOBuf> for Phase {
    type Error = ErrorKind;

    fn try_from(buf: iobuf::IOBuf) -> Result<Self, Self::Error> {
        let v: Vec<u8> = buf.into();
        match std::str::from_utf8(&v) {
            Ok("Draft") => Ok(Phase::Draft),
            Ok("Public") => Ok(Phase::Public),
            Ok(s) => Err(ErrorKind::PhasesError(
                format!("Conversion error from IOBuf to Phase for {}", s).into(),
            )),
            _ => Err(ErrorKind::PhasesError(
                format!(
                    "Conversion error from IOBuf to Phase, received {} bytes",
                    v.len()
                )
                .into(),
            )),
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

pub trait Phases: Send + Sync {
    /// mark all commits reachable from heads as public
    fn add_reachable_as_public(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        heads: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetId>, Error>;

    fn get_public(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<HashSet<ChangesetId>, Error>;

    fn is_public(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csid: ChangesetId,
    ) -> BoxFuture<bool, Error> {
        self.get_public(ctx, repo, vec![csid])
            .map(move |public| public.contains(&csid))
            .boxify()
    }
}

define_stats! {
    prefix = "mononoke.phases";
    get_single: timeseries(Rate, Sum),
    get_many: timeseries(Rate, Sum),
    add_many: timeseries(Rate, Sum),
}

queries! {
    write InsertPhase(values: (repo_id: RepositoryId, cs_id: ChangesetId, phase: Phase)) {
        none,
        mysql("INSERT INTO phases (repo_id, cs_id, phase) VALUES {values} ON DUPLICATE KEY UPDATE phase = VALUES(phase)")
        // sqlite query currently doesn't support changing the value
        // there is not usage for changing the phase at the moment
        // TODO (liubovd): improve sqlite query to make it semantically the same
        sqlite("INSERT OR IGNORE INTO phases (repo_id, cs_id, phase) VALUES {values}")
    }

    read SelectPhase(repo_id: RepositoryId, cs_id: ChangesetId) -> (Phase) {
        "SELECT phase FROM phases WHERE repo_id = {repo_id} AND cs_id = {cs_id}"
    }

    read SelectPhases(
        repo_id: RepositoryId,
        >list cs_ids: ChangesetId
    ) -> (ChangesetId, Phase) {
        "SELECT cs_id, phase
         FROM phases
         WHERE repo_id = {repo_id}
           AND cs_id IN {cs_ids}"
    }

    read SelectAllPublic(repo_id: RepositoryId) -> (ChangesetId, ) {
        "SELECT cs_id
         FROM phases
         WHERE repo_id = {repo_id}
           AND phase = 'Public'"
    }
}

#[derive(Clone)]
pub struct SqlPhases {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
}

impl SqlPhases {
    pub fn get_single_raw(
        &self,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> impl Future<Item = Option<Phase>, Error = Error> {
        STATS::get_single.add_value(1);
        SelectPhase::query(&self.read_connection, &repo_id, &cs_id)
            .map(move |rows| rows.into_iter().next().map(|row| row.0))
    }

    pub fn get_public_raw(
        &self,
        repo_id: RepositoryId,
        csids: &Vec<ChangesetId>,
    ) -> impl Future<Item = HashSet<ChangesetId>, Error = Error> {
        if csids.is_empty() {
            return future::ok(Default::default()).left_future();
        }
        STATS::get_many.add_value(1);
        SelectPhases::query(&self.read_connection, &repo_id, &csids)
            .map(move |rows| {
                rows.into_iter()
                    .filter(|row| row.1 == Phase::Public)
                    .map(|row| row.0)
                    .collect()
            })
            .right_future()
    }

    pub fn add_public(
        &self,
        _ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> impl Future<Item = (), Error = Error> {
        if csids.is_empty() {
            return future::ok(()).left_future();
        }
        let repoid = &repo.get_repoid();
        let phases: Vec<_> = csids
            .iter()
            .map(|csid| (repoid, csid, &Phase::Public))
            .collect();
        STATS::add_many.add_value(1);
        InsertPhase::query(&self.write_connection, &phases)
            .map(|_| ())
            .right_future()
    }

    pub fn list_all_public(
        &self,
        _ctx: CoreContext,
        repo_id: RepositoryId,
    ) -> impl Future<Item = Vec<ChangesetId>, Error = Error> {
        SelectAllPublic::query(&self.read_connection, &repo_id)
            .map(|ans| ans.into_iter().map(|x| x.0).collect())
    }

    pub fn get_public_derive(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
        ephemeral_derive: bool,
    ) -> BoxFuture<HashSet<ChangesetId>, Error> {
        if csids.is_empty() {
            return future::ok(Default::default()).boxify();
        }
        let repoid = repo.get_repoid();
        let this = self.clone();
        self.get_public_raw(repoid, &csids)
            .and_then(move |public_cold| {
                let mut unknown: Vec<_> = csids
                    .into_iter()
                    .filter(|csid| !public_cold.contains(csid))
                    .collect();
                if unknown.is_empty() {
                    return future::ok(public_cold).left_future();
                }
                repo.get_bonsai_heads_maybe_stale(ctx.clone())
                    .collect()
                    .and_then({
                        cloned!(this);
                        move |heads| {
                            mark_reachable_as_public(ctx, repo, this, &heads, ephemeral_derive)
                        }
                    })
                    .and_then(move |freshly_marked| {
                        // Still do the get_public_raw incase someone else marked the changes as public
                        // and thus mark_reachable_as_public did not return them as freshly_marked
                        this.get_public_raw(repoid, &unknown)
                            .map(move |public_hot| {
                                let public_combined = public_cold.into_iter().chain(public_hot);
                                if ephemeral_derive {
                                    unknown.retain(|e| freshly_marked.contains(e));
                                    public_combined.chain(unknown).collect()
                                } else {
                                    public_combined.collect()
                                }
                            })
                    })
                    .right_future()
            })
            .boxify()
    }
}

impl SqlConstructors for SqlPhases {
    const LABEL: &'static str = "phases";

    fn from_connections(
        write_connection: Connection,
        read_connection: Connection,
        read_master_connection: Connection,
    ) -> Self {
        Self {
            write_connection,
            read_connection,
            read_master_connection,
        }
    }

    fn get_up_query() -> &'static str {
        include_str!("../schemas/sqlite-phases.sql")
    }
}

impl Phases for SqlPhases {
    fn get_public(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        csids: Vec<ChangesetId>,
    ) -> BoxFuture<HashSet<ChangesetId>, Error> {
        self.get_public_derive(ctx, repo, csids, false)
    }

    fn add_reachable_as_public(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        heads: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetId>, Error> {
        mark_reachable_as_public(ctx, repo, self.clone(), &heads, false).boxify()
    }
}

/// Mark all commits reachable from `public_heads` as public
pub fn mark_reachable_as_public<'a, Heads>(
    ctx: CoreContext,
    repo: BlobRepo,
    phases: SqlPhases,
    public_heads: &'a Heads,
    ephemeral_derive: bool,
) -> impl Future<Item = Vec<ChangesetId>, Error = Error>
where
    &'a Heads: IntoIterator<Item = &'a ChangesetId>,
{
    let changeset_fetcher = repo.get_changeset_fetcher();
    let all_heads: Vec<_> = public_heads.into_iter().cloned().collect();
    phases
        .get_public_raw(repo.get_repoid(), &all_heads)
        .and_then({
            cloned!(ctx, repo, phases);
            move |public| {
                let input = all_heads
                    .into_iter()
                    .filter(|csid| !public.contains(csid))
                    .collect::<Vec<_>>();
                future::loop_fn((HashMap::new(), input), {
                    cloned!(ctx, repo, phases);
                    move |(mut output, mut input)| match input.pop() {
                        None => future::ok(Loop::Break(output)).left_future(),
                        Some(cs) => phases
                            .get_single_raw(repo.get_repoid(), cs)
                            .and_then({
                                cloned!(changeset_fetcher, ctx);
                                move |phase| match phase {
                                    Some(Phase::Public) => {
                                        future::ok(Loop::Continue((output, input))).left_future()
                                    }
                                    _ => (
                                        changeset_fetcher.get_generation_number(ctx.clone(), cs),
                                        changeset_fetcher.get_parents(ctx, cs),
                                    )
                                        .into_future()
                                        .map(move |(generation, parents)| {
                                            output.insert(cs, generation);
                                            input.extend(
                                                parents
                                                    .into_iter()
                                                    .filter(|p| !output.contains_key(p)),
                                            );
                                            Loop::Continue((output, input))
                                        })
                                        .right_future(),
                                }
                            })
                            .right_future(),
                    }
                })
            }
        })
        .and_then({
            move |unmarked| {
                // NOTE: We need to write phases in increasing generation number order, this will
                //       ensure that our phases in a valid state (i.e do not have any gaps). Once
                //       first public changeset is found we assume that all ancestors of it have
                //       already been marked as public.
                let mut unmarked: Vec<_> = unmarked.into_iter().map(|(k, v)| (v, k)).collect();
                unmarked.sort_by(|l, r| l.0.cmp(&r.0));
                let mark: Vec<_> = unmarked.into_iter().map(|(_gen, cs)| cs).collect();
                stream::iter_ok(mark.clone())
                    .chunks(100)
                    .and_then(move |chunk| {
                        if !ephemeral_derive {
                            phases
                                .add_public(ctx.clone(), repo.clone(), chunk)
                                .left_future()
                        } else {
                            future::ok(()).right_future()
                        }
                    })
                    .for_each(|()| future::ok(()))
                    .map(move |_| mark)
            }
        })
}
