// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

mod caching;
pub use caching::CachingHintPhases;
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
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::{fmt, str};
use try_from::TryFrom;

type FromValueResult<T> = ::std::result::Result<T, FromValueError>;

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
        match str::from_utf8(&v) {
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
    fn new(v: Value) -> FromValueResult<Self> {
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

#[derive(Debug, PartialEq, Default, Clone)]
pub struct PhasesMapping {
    pub calculated: HashMap<ChangesetId, Phase>,
    pub unknown: Vec<ChangesetId>,
    // filled if bookmarks are known or were fetched during calculation
    pub maybe_public_heads: Option<Arc<HashSet<ChangesetId>>>,
}

/// Interface to storage of phases in Mononoke
pub trait Phases: Send + Sync {
    /// Add a new entry to the phases.
    /// Returns true if a new changeset was added or the phase has been changed,
    /// returns false if the phase hasn't been changed for the changeset.
    fn add(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_id: ChangesetId,
        phase: Phase,
    ) -> BoxFuture<bool, Error>;

    /// Add new several entries to the phases.
    fn add_all(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        phases: Vec<(ChangesetId, Phase)>,
    ) -> BoxFuture<(), Error>;

    /// Retrieve the phase specified by this commit, if available.
    fn get(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<Phase>, Error>;

    /// Retrieve the phase for list of commits, if available.
    fn get_all(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_ids: Vec<ChangesetId>,
    ) -> BoxFuture<PhasesMapping, Error>;

    /// Retrieve the phase for list of commits, if available.
    /// Accept optional bookmarks. Use this API if bookmarks are known.
    fn get_all_with_bookmarks(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_ids: Vec<ChangesetId>,
        maybe_public_heads: Option<Arc<HashSet<ChangesetId>>>,
    ) -> BoxFuture<PhasesMapping, Error>;
}

#[derive(Clone)]
pub struct SqlPhases {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
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
    /// Add a new entry to the phases sql table. Returns true if a new changeset was inserted or the phase has been changed,
    /// returns false if the phase hasn't been changed for the changeset.
    fn add(
        &self,
        _ctx: CoreContext,
        repo: BlobRepo,
        cs_id: ChangesetId,
        phase: Phase,
    ) -> BoxFuture<bool, Error> {
        InsertPhase::query(
            &self.write_connection,
            &[(&repo.get_repoid(), &cs_id, &phase)],
        )
        .map(move |result| result.affected_rows() >= 1)
        .boxify()
    }

    /// Add new several entries to the phases.
    fn add_all(
        &self,
        _ctx: CoreContext,
        repo: BlobRepo,
        phases: Vec<(ChangesetId, Phase)>,
    ) -> BoxFuture<(), Error> {
        if phases.is_empty() {
            return future::ok(()).boxify();
        }
        let repo_id = &repo.get_repoid();
        InsertPhase::query(
            &self.write_connection,
            &phases
                .iter()
                .map(|(cs_id, phase)| (repo_id, cs_id, phase))
                .collect::<Vec<_>>(),
        )
        .map(|_| ())
        .boxify()
    }

    /// Retrieve the phase specified by this commit from the table, if available.
    fn get(
        &self,
        _ctx: CoreContext,
        repo: BlobRepo,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<Phase>, Error> {
        SelectPhase::query(&self.read_connection, &repo.get_repoid(), &cs_id)
            .map(move |rows| rows.into_iter().next().map(|row| row.0))
            .boxify()
    }

    /// Retrieve the phase for list of commits, if available.
    fn get_all(
        &self,
        _ctx: CoreContext,
        repo: BlobRepo,
        cs_ids: Vec<ChangesetId>,
    ) -> BoxFuture<PhasesMapping, Error> {
        if cs_ids.is_empty() {
            return future::ok(Default::default()).boxify();
        }
        SelectPhases::query(
            &self.read_connection,
            &repo.get_repoid(),
            &cs_ids.iter().collect::<Vec<_>>(),
        )
        .map(move |rows| {
            let calculated = rows
                .into_iter()
                .map(|row| (row.0, row.1))
                .collect::<HashMap<_, _>>();
            let unknown = cs_ids
                .into_iter()
                .filter(|cs_id| !calculated.contains_key(cs_id))
                .collect();
            PhasesMapping {
                calculated,
                unknown,
                ..Default::default()
            }
        })
        .boxify()
    }

    /// Retrieve the phase for list of commits, if available.
    /// Accept optional bookmarks. Use this API if bookmarks are known.
    /// Bookmarks are not used, pass them as is.
    fn get_all_with_bookmarks(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_ids: Vec<ChangesetId>,
        maybe_public_heads: Option<Arc<HashSet<ChangesetId>>>,
    ) -> BoxFuture<PhasesMapping, Error> {
        self.get_all(ctx, repo, cs_ids)
            .map(|mut phases_mapping| {
                phases_mapping.maybe_public_heads = maybe_public_heads;
                phases_mapping
            })
            .boxify()
    }
}

pub struct HintPhases {
    phases_store: Arc<dyn Phases>, // phases_store is the underlying persistent storage (db)
}

impl HintPhases {
    pub fn new(phases_store: Arc<dyn Phases>) -> Self {
        Self { phases_store }
    }
}

impl Phases for HintPhases {
    /// Add a new phases entry to the underlying storage.
    /// Returns true if a new changeset was added or the phase has been changed,
    /// returns false if the phase hasn't been changed for the changeset.
    fn add(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_id: ChangesetId,
        phase: Phase,
    ) -> BoxFuture<bool, Error> {
        // Refresh the underlying persistent storage (currently for public commits only).
        if phase == Phase::Public {
            self.phases_store.add(ctx, repo, cs_id, phase)
        } else {
            future::ok(false).boxify()
        }
    }

    /// Add several new entries to the underlying storage.
    fn add_all(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        phases: Vec<(ChangesetId, Phase)>,
    ) -> BoxFuture<(), Error> {
        // Refresh the underlying persistent storage (currently for public commits only).
        self.phases_store.add_all(
            ctx,
            repo,
            phases
                .into_iter()
                .filter(|(_, phase)| phase == &Phase::Public)
                .collect(),
        )
    }

    /// Retrieve the phase specified by this commit, if available.
    /// If phases are not available for some of the commits, they will be recalculated.
    /// If recalculation failed error will be returned.
    fn get(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<Phase>, Error> {
        self.get_all(ctx, repo, vec![cs_id])
            .map(move |mut phases_mapping| phases_mapping.calculated.remove(&cs_id))
            .boxify()
    }

    /// Get phases for the list of commits.
    /// If phases are not available for some of the commits, they will be recalculated.
    /// If recalculation failed error will be returned.
    /// Uknown is always returned empty.
    fn get_all(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_ids: Vec<ChangesetId>,
    ) -> BoxFuture<PhasesMapping, Error> {
        self.get_all_with_bookmarks(ctx, repo, cs_ids, None)
    }

    /// Get phases for the list of commits.
    /// Accept optional bookmarks heads. Use this API if bookmarks are known.
    /// If phases are not available for some of the commits, they will be recalculated.
    /// If recalculation failed an error will be returned.
    /// Returns:
    /// phases_mapping::calculated          - phases hash map
    /// phases_mapping::unknown             - always empty
    /// phases_mapping::maybe_public_heads  - if bookmarks heads were fetched during calculation
    ///                                   or passed to this function they will be filled in.
    fn get_all_with_bookmarks(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_ids: Vec<ChangesetId>,
        maybe_public_heads: Option<Arc<HashSet<ChangesetId>>>,
    ) -> BoxFuture<PhasesMapping, Error> {
        cloned!(self.phases_store);
        phases_store
            .get_all_with_bookmarks(
                ctx.clone(),
                repo.clone(),
                cs_ids,
                maybe_public_heads.clone(),
            )
            .and_then(move |phases| {
                fill_unkown_phases(ctx, repo, phases_store, maybe_public_heads, phases)
            })
            .boxify()
    }
}

// resolve unknown phases and return update result
fn fill_unkown_phases(
    ctx: CoreContext,
    repo: BlobRepo,
    phases_store: Arc<dyn Phases>,
    maybe_public_heads: Option<Arc<HashSet<ChangesetId>>>,
    phases: PhasesMapping,
) -> impl Future<Item = PhasesMapping, Error = Error> {
    if phases.unknown.is_empty() {
        return future::ok(PhasesMapping {
            maybe_public_heads,
            ..phases
        })
        .left_future();
    }

    let PhasesMapping {
        calculated: calculated_input,
        unknown,
        ..
    } = phases;
    match maybe_public_heads {
        Some(public_heads) => future::ok(public_heads).left_future(),
        None => repo
            .get_bonsai_heads(ctx.clone())
            .map(|(_, cs_id)| cs_id)
            .collect()
            .map(move |bookmarks| Arc::new(bookmarks.into_iter().collect()))
            .right_future(),
    }
    .and_then(move |public_heads| {
        mark_reachable_as_public(
            ctx.clone(),
            repo.clone(),
            phases_store.clone(),
            &*public_heads,
        )
        .and_then(move |_| phases_store.get_all(ctx, repo, unknown))
        .map(move |phases| {
            let PhasesMapping {
                mut calculated,
                unknown,
                ..
            } = phases;
            calculated.extend(calculated_input);
            calculated.extend(unknown.into_iter().map(|cs| (cs, Phase::Draft)));
            PhasesMapping {
                calculated,
                unknown: Vec::new(),
                maybe_public_heads: Some(public_heads),
            }
        })
    })
    .right_future()
}

/// Mark all commits reachable from `public_heads` as public
pub fn mark_reachable_as_public<'a, Heads>(
    ctx: CoreContext,
    repo: BlobRepo,
    phases_store: Arc<dyn Phases>,
    public_heads: &'a Heads,
) -> impl Future<Item = (), Error = Error>
where
    &'a Heads: IntoIterator<Item = &'a ChangesetId>,
{
    let changeset_fetcher = repo.get_changeset_fetcher();
    let input: Vec<_> = public_heads.into_iter().cloned().collect();
    future::loop_fn((HashMap::new(), input), {
        cloned!(ctx, repo, phases_store);
        move |(mut output, mut input)| match input.pop() {
            None => future::ok(Loop::Break(output)).left_future(),
            Some(cs) => phases_store
                .get(ctx.clone(), repo.clone(), cs)
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
                                    parents.into_iter().filter(|p| !output.contains_key(p)),
                                );
                                Loop::Continue((output, input))
                            })
                            .right_future(),
                    }
                })
                .right_future(),
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
            stream::iter_ok(unmarked)
                .chunks(100)
                .and_then(move |chunk| {
                    phases_store.add_all(
                        ctx.clone(),
                        repo.clone(),
                        chunk.iter().map(|(_, cs)| (*cs, Phase::Public)).collect(),
                    )
                })
                .for_each(|()| future::ok(()))
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bookmarks::{Bookmark, BookmarkUpdateReason};
    use fixtures::linear;
    use futures::Stream;
    use maplit::{hashmap, hashset};
    use mercurial_types::nodehash::HgChangesetId;
    use mononoke_types_mocks::changesetid::*;
    use std::str::FromStr;
    use tokio::runtime::Runtime;

    fn add_get_sql_phase<P: Phases>(phases: P) {
        let ctx = CoreContext::test_mock();
        let repo = blobrepo_factory::new_memblob_empty(None, None).unwrap();

        assert_eq!(
            phases
                .add(ctx.clone(), repo.clone(), ONES_CSID, Phase::Public)
                .wait()
                .expect("Adding new phase entry failed"),
            true,
            "sql: try to add phase Public for a new changeset"
        );

        assert_eq!(
            phases
                .add(ctx.clone(), repo.clone(), ONES_CSID, Phase::Public)
                .wait()
                .expect("Adding new phase entry failed"),
            false,
            "sql: try to add the same changeset with the same phase"
        );

        assert_eq!(
            phases
                .get(ctx.clone(), repo.clone(), ONES_CSID)
                .wait()
                .expect("Get phase failed"),
            Some(Phase::Public),
            "sql: get phase for the existing changeset"
        );

        assert_eq!(
            phases
                .get(ctx.clone(), repo.clone(), TWOS_CSID)
                .wait()
                .expect("Get phase failed"),
            None,
            "sql: get phase for non existing changeset"
        );

        assert_eq!(
            phases
                .get_all(ctx.clone(), repo.clone(), vec![ONES_CSID, TWOS_CSID])
                .wait()
                .expect("Get phase failed"),
            PhasesMapping {
                calculated: hashmap! {ONES_CSID => Phase::Public},
                unknown: vec![TWOS_CSID],
                ..Default::default()
            },
            "sql: get phase for non existing changeset and existing changeset"
        );
    }

    #[test]
    fn add_get_phase_sql_test() {
        async_unit::tokio_unit_test(|| {
            add_get_sql_phase(SqlPhases::with_sqlite_in_memory().unwrap());
        });
    }

    fn delete_bookmark(ctx: CoreContext, repo: BlobRepo, book: &Bookmark) {
        let mut txn = repo.update_bookmark_transaction(ctx);
        txn.force_delete(
            &book,
            BookmarkUpdateReason::TestMove {
                bundle_replay_data: None,
            },
        )
        .unwrap();
        txn.commit().wait().unwrap();
    }

    fn set_bookmark(ctx: CoreContext, repo: BlobRepo, book: &Bookmark, cs_id: &str) {
        let head = repo
            .get_bonsai_from_hg(ctx.clone(), HgChangesetId::from_str(cs_id).unwrap())
            .wait()
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
        txn.commit().wait().unwrap();
    }

    /*
            @  79a13814c5ce7330173ec04d279bf95ab3f652fb
            |
            o  a5ffa77602a066db7d5cfb9fb5823a0895717c5a
            |
            o  3c15267ebf11807f3d772eb891272b911ec68759
            |
            o  a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157
            |
            o  0ed509bf086fadcb8a8a5384dc3b550729b0fc17
            |
            o  eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b  master
            |
            o  cb15ca4a43a59acff5388cea9648c162afde8372
            |
            o  d0a361e9022d226ae52f689667bd7d212a19cfe0
            |
            o  607314ef579bd2407752361ba1b0c1729d08b281
            |
            o  3e0e761030db6e479a7fb58b12881883f9f8c63f
            |
            o  2d7d4ba9ce0a6ffd222de7785b249ead9c51c536
    */

    fn get_hint_phase() {
        let ctx = CoreContext::test_mock();
        let repo = linear::getrepo(None);

        // delete all existing bookmarks
        for (bookmark, _) in repo
            .get_bonsai_bookmarks(ctx.clone())
            .collect()
            .wait()
            .unwrap()
        {
            delete_bookmark(ctx.clone(), repo.clone(), &bookmark);
        }

        // create a new master bookmark
        set_bookmark(
            ctx.clone(),
            repo.clone(),
            &Bookmark::new("master").unwrap(),
            "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b",
        );

        let public_commit = repo
            .get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("d0a361e9022d226ae52f689667bd7d212a19cfe0").unwrap(),
            )
            .wait()
            .unwrap()
            .unwrap();

        let other_public_commit = repo
            .get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").unwrap(),
            )
            .wait()
            .unwrap()
            .unwrap();

        let draft_commit = repo
            .get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("a9473beb2eb03ddb1cccc3fbaeb8a4820f9cd157").unwrap(),
            )
            .wait()
            .unwrap()
            .unwrap();

        let other_draft_commit = repo
            .get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("a5ffa77602a066db7d5cfb9fb5823a0895717c5a").unwrap(),
            )
            .wait()
            .unwrap()
            .unwrap();

        let public_bookmark_commit = repo
            .get_bonsai_from_hg(
                ctx.clone(),
                HgChangesetId::from_str("eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b").unwrap(),
            )
            .wait()
            .unwrap()
            .unwrap();

        let phases_store = Arc::new(SqlPhases::with_sqlite_in_memory().unwrap());
        let hint_phases = HintPhases::new(phases_store.clone());

        assert_eq!(
            hint_phases
                .get(ctx.clone(), repo.clone(), public_bookmark_commit)
                .wait()
                .unwrap(),
            Some(Phase::Public),
            "slow path: get phase for a Public commit which is also a public bookmark"
        );

        assert_eq!(
            hint_phases
                .get(ctx.clone(), repo.clone(), public_commit)
                .wait()
                .unwrap(),
            Some(Phase::Public),
            "slow path: get phase for a Public commit"
        );

        assert_eq!(
            hint_phases
                .get(ctx.clone(), repo.clone(), other_public_commit)
                .wait()
                .unwrap(),
            Some(Phase::Public),
            "slow path: get phase for other Public commit"
        );

        assert_eq!(
            hint_phases
                .get(ctx.clone(), repo.clone(), draft_commit)
                .wait()
                .unwrap(),
            Some(Phase::Draft),
            "slow path: get phase for a Draft commit"
        );

        assert_eq!(
            hint_phases
                .get(ctx.clone(), repo.clone(), other_draft_commit)
                .wait()
                .unwrap(),
            Some(Phase::Draft),
            "slow path: get phase for other Draft commit"
        );

        assert_eq!(
            hint_phases
                .get_all(
                    ctx.clone(),
                    repo.clone(),
                    vec![
                        public_commit,
                        other_public_commit,
                        draft_commit,
                        other_draft_commit
                    ]
                )
                .wait()
                .unwrap(),
            PhasesMapping {
                calculated: hashmap! {
                    public_commit => Phase::Public,
                    other_public_commit => Phase::Public,
                    draft_commit => Phase::Draft,
                    other_draft_commit => Phase::Draft
                },
                unknown: vec![],
                maybe_public_heads: Some(Arc::new(hashset! {
                    public_bookmark_commit
                }))
            },
            "slow path: get phases for set of commits"
        );

        assert_eq!(
            phases_store
                .get(ctx.clone(), repo.clone(), public_commit)
                .wait()
                .expect("Get phase failed"),
            Some(Phase::Public),
            "sql: make sure that phase was written to the db for public commit"
        );

        assert_eq!(
            phases_store
                .get(ctx.clone(), repo.clone(), draft_commit)
                .wait()
                .expect("Get phase failed"),
            None,
            "sql: make sure that phase was not written to the db for draft commit"
        );
    }

    #[test]
    fn get_phase_hint_test() {
        async_unit::tokio_unit_test(|| {
            get_hint_phase();
        });
    }

    #[test]
    fn test_mark_reachable_as_public() {
        let mut rt = Runtime::new().unwrap();

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

        // delete all existing bookmarks
        let bookmarks = rt
            .block_on(repo.get_bonsai_bookmarks(ctx.clone()).collect())
            .unwrap();
        let mut transaction = repo.update_bookmark_transaction(ctx.clone());
        for bookmark in bookmarks {
            transaction
                .force_delete(
                    &bookmark.0,
                    BookmarkUpdateReason::TestMove {
                        bundle_replay_data: None,
                    },
                )
                .unwrap();
        }
        assert!(rt.block_on(transaction.commit()).unwrap());

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

        let phases: Arc<dyn Phases> = Arc::new(SqlPhases::with_sqlite_in_memory().unwrap());
        // get phases mapping for all `bcss` in the same order
        let get_phases_map = || {
            phases
                .get_all(ctx.clone(), repo.clone(), bcss.clone())
                .map({
                    cloned!(bcss);
                    move |mapping| {
                        bcss.iter()
                            .map(|bcs| {
                                mapping
                                    .calculated
                                    .get(bcs)
                                    .map_or(false, |phase| phase == &Phase::Public)
                            })
                            .collect::<Vec<_>>()
                    }
                })
        };

        // all phases are draft
        assert_eq!(rt.block_on(get_phases_map()).unwrap(), [false; 7]);

        rt.block_on(mark_reachable_as_public(
            ctx.clone(),
            repo.clone(),
            phases.clone(),
            &[bcss[1]],
        ))
        .unwrap();
        assert_eq!(
            rt.block_on(get_phases_map()).unwrap(),
            [true, true, false, false, false, false, false],
        );

        rt.block_on(mark_reachable_as_public(
            ctx.clone(),
            repo.clone(),
            phases.clone(),
            &[bcss[2], bcss[5]],
        ))
        .unwrap();
        assert_eq!(
            rt.block_on(get_phases_map()).unwrap(),
            [true, true, true, false, true, true, false],
        );
    }
}
