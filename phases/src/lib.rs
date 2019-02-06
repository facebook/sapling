// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate ascii;
extern crate blobrepo;
#[cfg(test)]
extern crate blobrepo_factory;
extern crate changeset_fetcher;
#[macro_use]
extern crate cloned;
extern crate context;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
extern crate futures_ext;
extern crate iobuf;
extern crate memcache;
extern crate mercurial_types;
extern crate mononoke_types;
extern crate reachabilityindex;
extern crate skiplist;
extern crate tokio;
extern crate try_from;
#[cfg(test)]
#[macro_use]
extern crate maplit;

#[macro_use]
extern crate sql;
extern crate sql_ext;

#[macro_use]
extern crate stats;

mod caching;
pub use caching::CachingHintPhases;

mod errors;
pub use errors::*;

mod hint;
pub use hint::PhasesReachabilityHint;

use ascii::AsciiString;
use blobrepo::BlobRepo;
use context::CoreContext;
use futures::{future, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::HgPhase;
use mononoke_types::{ChangesetId, RepositoryId};
use skiplist::SkiplistIndex;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::{fmt, str};
use try_from::TryFrom;

use sql::mysql_async::{
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};
use sql::Connection;
pub use sql_ext::SqlConstructors;

type FromValueResult<T> = ::std::result::Result<T, FromValueError>;

#[derive(Clone, PartialEq, Debug)]
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
            Phase::Draft => write!(f, "{}", "Draft"),
            Phase::Public => write!(f, "{}", "Public"),
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

#[derive(Debug, PartialEq, Default)]
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

pub struct HintPhases {
    phases_store: Arc<Phases>, // phases_store is the underlying persistent storage (db)
    phases_reachability_hint: PhasesReachabilityHint, // phases_reachability_hint for slow path calculation
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

impl HintPhases {
    pub fn new(phases_store: Arc<Phases>, skip_index: Arc<SkiplistIndex>) -> Self {
        Self {
            phases_store,
            phases_reachability_hint: PhasesReachabilityHint::new(skip_index),
        }
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
        cloned!(self.phases_store, self.phases_reachability_hint);
        // Try to fetch from the underlying storage.
        phases_store
            .get_all(ctx.clone(), repo.clone(), cs_ids)
            .and_then(move |phases_mapping| {
                // For not found part calculate phases using the phases_reachability_hint.
                let not_found_in_db = phases_mapping.unknown;
                let found_in_db = phases_mapping.calculated;

                // Public heads are required (only if not_found_in_db is not empty).
                // Fetch them once or reuse known.
                // Pass them to the response, so they can be reused.
                let public_heads_fut = if maybe_public_heads.is_some() {
                    future::ok(maybe_public_heads).boxify() // known
                } else if not_found_in_db.is_empty() {
                    future::ok(None).boxify() // not needed
                } else {
                    repo.get_bonsai_bookmarks(ctx.clone()) // calculate
                        .map(|(_, cs_id)| cs_id)
                        .collect()
                        .map(move |bookmarks| Some(Arc::new(bookmarks.into_iter().collect())))
                        .boxify()
                };

                let calculated_fut = {
                    cloned!(ctx, repo);
                    public_heads_fut.and_then(move |maybe_public_heads| {
                        if let Some(ref public_heads) = maybe_public_heads {
                            phases_reachability_hint
                                .get_all(
                                    ctx,
                                    repo.get_changeset_fetcher(),
                                    not_found_in_db,
                                    public_heads.clone(),
                                )
                                .left_future()
                        } else {
                            future::ok(HashMap::new()).right_future()
                        }
                        .map(move |calculated| (calculated, maybe_public_heads))
                    })
                };

                calculated_fut.and_then(move |(mut calculated, maybe_public_heads)| {
                    // Refresh newly calculated phases in the underlying storage (public commits only).
                    let add_to_db = calculated
                        .iter()
                        .filter_map(|(cs_id, phase)| {
                            if phase == &Phase::Public {
                                Some((cs_id.clone(), phase.clone()))
                            } else {
                                None
                            }
                        })
                        .collect();

                    // Join the found in the db and calculated into the returned result.
                    calculated.extend(found_in_db);

                    phases_store
                        .add_all(ctx, repo, add_to_db)
                        .map(move |_| PhasesMapping {
                            calculated,
                            unknown: vec![],
                            maybe_public_heads,
                        })
                })
            })
            .boxify()
    }
}

#[cfg(test)]
mod tests {
    extern crate async_unit;
    extern crate bookmarks;
    extern crate fixtures;
    extern crate mononoke_types_mocks;

    use super::*;
    use futures::Stream;
    use mercurial_types::nodehash::HgChangesetId;
    use std::str::FromStr;
    use tests::bookmarks::Bookmark;
    use tests::fixtures::linear;
    use tests::mononoke_types_mocks::changesetid::*;

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
        txn.force_delete(&book).unwrap();
        txn.commit().wait().unwrap();
    }

    fn set_bookmark(ctx: CoreContext, repo: BlobRepo, book: &Bookmark, cs_id: &str) {
        let head = repo
            .get_bonsai_from_hg(ctx.clone(), HgChangesetId::from_str(cs_id).unwrap())
            .wait()
            .unwrap()
            .unwrap();
        let mut txn = repo.update_bookmark_transaction(ctx);
        txn.force_set(&book, head).unwrap();
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
        let hint_phases = HintPhases::new(phases_store.clone(), Arc::new(SkiplistIndex::new()));

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
}
