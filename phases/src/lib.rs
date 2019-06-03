// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

mod caching;
pub use caching::CachingPhases;
mod errors;
pub use errors::*;

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
use stats::{define_stats, Timeseries};
use std::{
    collections::{HashMap, HashSet},
    fmt,
};
use try_from::TryFrom;

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
    type Err = ErrorKind;

    fn try_from(buf: iobuf::IOBuf) -> ::std::result::Result<Self, Self::Err> {
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
    fn new(v: Value) -> ::std::result::Result<Self, FromValueError> {
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
    get_single: timeseries(RATE, SUM),
    get_many: timeseries(RATE, SUM),
    add_many: timeseries(RATE, SUM),
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
}

#[derive(Clone)]
pub struct SqlPhases {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
}

impl SqlPhases {
    fn get_single_raw(
        &self,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> impl Future<Item = Option<Phase>, Error = Error> {
        STATS::get_single.add_value(1);
        SelectPhase::query(&self.read_connection, &repo_id, &cs_id)
            .map(move |rows| rows.into_iter().next().map(|row| row.0))
    }

    fn get_public_raw(
        &self,
        repo_id: RepositoryId,
        csids: &Vec<ChangesetId>,
    ) -> impl Future<Item = HashSet<ChangesetId>, Error = Error> {
        if csids.is_empty() {
            return future::ok(Default::default()).left_future();
        }
        STATS::get_many.add_value(1);
        SelectPhases::query(
            &self.read_connection,
            &repo_id,
            &csids.iter().collect::<Vec<_>>(),
        )
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
        if csids.is_empty() {
            return future::ok(Default::default()).boxify();
        }
        let repoid = repo.get_repoid();
        let this = self.clone();
        self.get_public_raw(repoid, &csids)
            .and_then(move |public_cold| {
                let unknown: Vec<_> = csids
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
                        move |heads| mark_reachable_as_public(ctx, repo, this, &heads)
                    })
                    .and_then(move |_| {
                        this.get_public_raw(repoid, &unknown)
                            .map(move |public_hot| {
                                public_cold.into_iter().chain(public_hot).collect()
                            })
                    })
                    .right_future()
            })
            .boxify()
    }

    fn add_reachable_as_public(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        heads: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetId>, Error> {
        mark_reachable_as_public(ctx, repo, self.clone(), &heads).boxify()
    }
}

/// Mark all commits reachable from `public_heads` as public
fn mark_reachable_as_public<'a, Heads>(
    ctx: CoreContext,
    repo: BlobRepo,
    phases: SqlPhases,
    public_heads: &'a Heads,
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
                    .and_then(move |chunk| phases.add_public(ctx.clone(), repo.clone(), chunk))
                    .for_each(|()| future::ok(()))
                    .map(move |_| mark)
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bookmarks::{BookmarkName, BookmarkUpdateReason};
    use fixtures::linear;
    use futures::Stream;
    use maplit::hashset;
    use mercurial_types::nodehash::HgChangesetId;
    use mononoke_types_mocks::changesetid::*;
    use std::str::FromStr;
    use tokio::runtime::Runtime;

    #[test]
    fn add_get_phase_sql_test() -> Result<()> {
        let mut rt = Runtime::new()?;
        let ctx = CoreContext::test_mock();
        let repo = blobrepo_factory::new_memblob_empty(None, None)?;
        let phases = SqlPhases::with_sqlite_in_memory()?;

        rt.block_on(phases.add_public(ctx.clone(), repo.clone(), vec![ONES_CSID]))?;

        assert_eq!(
            rt.block_on(phases.is_public(ctx.clone(), repo.clone(), ONES_CSID))?,
            true,
            "sql: get phase for the existing changeset"
        );

        assert_eq!(
            rt.block_on(phases.is_public(ctx.clone(), repo.clone(), TWOS_CSID))?,
            false,
            "sql: get phase for non existing changeset"
        );

        assert_eq!(
            rt.block_on(phases.get_public(ctx.clone(), repo.clone(), vec![ONES_CSID, TWOS_CSID]))?,
            hashset! {ONES_CSID},
            "sql: get phase for non existing changeset and existing changeset"
        );

        Ok(())
    }

    fn delete_all_publishing_bookmarks(rt: &mut Runtime, ctx: CoreContext, repo: BlobRepo) {
        let bookmarks = rt
            .block_on(
                repo.get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
                    .collect(),
            )
            .unwrap();

        let mut txn = repo.update_bookmark_transaction(ctx);

        for (bookmark, _) in bookmarks {
            txn.force_delete(
                bookmark.name(),
                BookmarkUpdateReason::TestMove {
                    bundle_replay_data: None,
                },
            )
            .unwrap();
        }

        assert!(rt.block_on(txn.commit()).unwrap());
    }

    fn set_bookmark(
        rt: &mut Runtime,
        ctx: CoreContext,
        repo: BlobRepo,
        book: &BookmarkName,
        cs_id: &str,
    ) {
        let head = rt
            .block_on(repo.get_bonsai_from_hg(ctx.clone(), HgChangesetId::from_str(cs_id).unwrap()))
            .unwrap()
            .unwrap();
        let mut txn = repo.update_bookmark_transaction(ctx);
        txn.force_set(
            &book,
            head,
            BookmarkUpdateReason::TestMove {
                bundle_replay_data: None,
            },
        )
        .unwrap();

        assert!(rt.block_on(txn.commit()).unwrap());
    }

    #[test]
    fn get_phase_hint_test() {
        let mut rt = Runtime::new().unwrap();

        let repo = linear::getrepo(None);
        //  @  79a13814c5ce7330173ec04d279bf95ab3f652fb
        //  |
        //  o  a5ffa77602a066db7d5cfb9fb5823a0895717c5a
        //  |
        //  o  3c15267ebf11807f3d772eb891272b911ec68759
        //  |
        //  o  a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157
        //  |
        //  o  0ed509bf086fadcb8a8a5384dc3b550729b0fc17
        //  |
        //  o  eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b  master
        //  |
        //  o  cb15ca4a43a59acff5388cea9648c162afde8372
        //  |
        //  o  d0a361e9022d226ae52f689667bd7d212a19cfe0
        //  |
        //  o  607314ef579bd2407752361ba1b0c1729d08b281
        //  |
        //  o  3e0e761030db6e479a7fb58b12881883f9f8c63f
        //  |
        //  o  2d7d4ba9ce0a6ffd222de7785b249ead9c51c536

        let ctx = CoreContext::test_mock();

        delete_all_publishing_bookmarks(&mut rt, ctx.clone(), repo.clone());

        // create a new master bookmark
        set_bookmark(
            &mut rt,
            ctx.clone(),
            repo.clone(),
            &BookmarkName::new("master").unwrap(),
            "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b",
        );

        let public_commit = rt
            .block_on(repo.get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("d0a361e9022d226ae52f689667bd7d212a19cfe0").unwrap(),
            ))
            .unwrap()
            .unwrap();

        let other_public_commit = rt
            .block_on(repo.get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
            ))
            .unwrap()
            .unwrap();

        let draft_commit = rt
            .block_on(repo.get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").unwrap(),
            ))
            .unwrap()
            .unwrap();

        let other_draft_commit = rt
            .block_on(repo.get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a").unwrap(),
            ))
            .unwrap()
            .unwrap();

        let public_bookmark_commit = rt
            .block_on(repo.get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b").unwrap(),
            ))
            .unwrap()
            .unwrap();

        let phases = SqlPhases::with_sqlite_in_memory().unwrap();

        assert_eq!(
            rt.block_on(phases.is_public(ctx.clone(), repo.clone(), public_bookmark_commit))
                .unwrap(),
            true,
            "slow path: get phase for a Public commit which is also a public bookmark"
        );

        assert_eq!(
            rt.block_on(phases.is_public(ctx.clone(), repo.clone(), public_commit))
                .unwrap(),
            true,
            "slow path: get phase for a Public commit"
        );

        assert_eq!(
            rt.block_on(phases.is_public(ctx.clone(), repo.clone(), other_public_commit))
                .unwrap(),
            true,
            "slow path: get phase for other Public commit"
        );

        assert_eq!(
            rt.block_on(phases.is_public(ctx.clone(), repo.clone(), draft_commit))
                .unwrap(),
            false,
            "slow path: get phase for a Draft commit"
        );

        assert_eq!(
            rt.block_on(phases.is_public(ctx.clone(), repo.clone(), other_draft_commit))
                .unwrap(),
            false,
            "slow path: get phase for other Draft commit"
        );

        assert_eq!(
            rt.block_on(phases.get_public(
                ctx.clone(),
                repo.clone(),
                vec![
                    public_commit,
                    other_public_commit,
                    draft_commit,
                    other_draft_commit
                ]
            ))
            .unwrap(),
            hashset! {
                public_commit,
                other_public_commit,
            },
            "slow path: get phases for set of commits"
        );

        assert_eq!(
            rt.block_on(phases.is_public(ctx.clone(), repo.clone(), public_commit))
                .expect("Get phase failed"),
            true,
            "sql: make sure that phase was written to the db for public commit"
        );

        assert_eq!(
            rt.block_on(phases.is_public(ctx.clone(), repo.clone(), draft_commit))
                .expect("Get phase failed"),
            false,
            "sql: make sure that phase was not written to the db for draft commit"
        );
    }

    #[test]
    fn test_mark_reachable_as_public() -> Result<()> {
        let mut rt = Runtime::new()?;

        let repo = fixtures::branch_even::getrepo(None);
        // @  4f7f3fd428bec1a48f9314414b063c706d9c1aed (6)
        // |
        // o  b65231269f651cfe784fd1d97ef02a049a37b8a0 (5)
        // |
        // o  d7542c9db7f4c77dab4b315edd328edf1514952f (4)
        // |
        // | o  16839021e338500b3cf7c9b871c8a07351697d68 (3)
        // | |
        // | o  1d8a907f7b4bf50c6a09c16361e2205047ecc5e5 (2)
        // | |
        // | o  3cda5c78aa35f0f5b09780d971197b51cad4613a (1)
        // |/
        // |
        // o  15c40d0abc36d47fb51c8eaec51ac7aad31f669c (0)
        let hgcss = [
            "15c40d0abc36d47fb51c8eaec51ac7aad31f669c",
            "3cda5c78aa35f0f5b09780d971197b51cad4613a",
            "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5",
            "16839021e338500b3cf7c9b871c8a07351697d68",
            "d7542c9db7f4c77dab4b315edd328edf1514952f",
            "b65231269f651cfe784fd1d97ef02a049a37b8a0",
            "4f7f3fd428bec1a48f9314414b063c706d9c1aed",
        ];
        let ctx = CoreContext::test_mock();

        delete_all_publishing_bookmarks(&mut rt, ctx.clone(), repo.clone());

        // resolve bonsai
        let bcss = rt
            .block_on(future::join_all(
                hgcss
                    .iter()
                    .map(|hgcs| {
                        repo.get_bonsai_from_hg(ctx.clone(), HgChangesetId::from_str(hgcs).unwrap())
                            .map(|bcs| bcs.unwrap())
                    })
                    .collect::<Vec<_>>(),
            ))
            .unwrap();

        let phases = SqlPhases::with_sqlite_in_memory()?;
        // get phases mapping for all `bcss` in the same order
        let get_phases_map = || {
            phases
                .get_public(ctx.clone(), repo.clone(), bcss.clone())
                .map({
                    cloned!(bcss);
                    move |public| {
                        bcss.iter()
                            .map(|bcs| public.contains(bcs))
                            .collect::<Vec<_>>()
                    }
                })
        };

        // all phases are draft
        assert_eq!(rt.block_on(get_phases_map())?, [false; 7]);

        rt.block_on(mark_reachable_as_public(
            ctx.clone(),
            repo.clone(),
            phases.clone(),
            &[bcss[1]],
        ))?;
        assert_eq!(
            rt.block_on(get_phases_map())?,
            [true, true, false, false, false, false, false],
        );

        rt.block_on(mark_reachable_as_public(
            ctx.clone(),
            repo.clone(),
            phases.clone(),
            &[bcss[2], bcss[5]],
        ))?;
        assert_eq!(
            rt.block_on(get_phases_map())?,
            [true, true, true, false, true, true, false],
        );

        Ok(())
    }
}
