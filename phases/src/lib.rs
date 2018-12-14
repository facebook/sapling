// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate ascii;
extern crate blobrepo;
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
extern crate tokio;
extern crate try_from;

#[macro_use]
extern crate sql;
extern crate sql_ext;

#[macro_use]
extern crate stats;

mod caching;
pub use caching::CachingPhases;

mod errors;
pub use errors::*;

mod hint;
pub use hint::PhasesHint;

use ascii::AsciiString;
use blobrepo::BlobRepo;
use context::CoreContext;
use futures::Future;
use futures_ext::BoxFuture;
use futures_ext::FutureExt;
use mercurial_types::RepositoryId;
use mononoke_types::ChangesetId;
use std::fmt;
use try_from::TryFrom;

use sql::Connection;
use sql::mysql_async::{FromValueError, Value, prelude::{ConvIr, FromValue}};
pub use sql_ext::SqlConstructors;

use std::str;

type FromValueResult<T> = ::std::result::Result<T, FromValueError>;

#[derive(Clone, PartialEq)]
pub enum Phase {
    Draft,
    Public,
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
                ).into(),
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

    /// Retrieve the phase specified by this commit, if available.
    fn get(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<Phase>, Error>;
}

#[derive(Clone)]
pub struct SqlPhases {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
}

queries!{
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
        ).map(move |result| result.affected_rows() >= 1)
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
}

#[cfg(test)]
mod tests {
    extern crate async_unit;
    extern crate mononoke_types_mocks;

    use super::*;
    use tests::mononoke_types_mocks::changesetid::*;

    fn add_get_phase<P: Phases>(phases: P) {
        let ctx = CoreContext::test_mock();
        let repo = BlobRepo::new_memblob_empty(None, None).unwrap();

        assert_eq!(
            phases
                .add(ctx.clone(), repo.clone(), ONES_CSID, Phase::Public)
                .wait()
                .expect("Adding new phase entry failed"),
            true,
            "try to add phase Public for a new changeset"
        );

        assert_eq!(
            phases
                .add(ctx.clone(), repo.clone(), ONES_CSID, Phase::Public)
                .wait()
                .expect("Adding new phase entry failed"),
            false,
            "try to add the same changeset with the same phase"
        );

        assert_eq!(
            phases
                .get(ctx.clone(), repo.clone(), ONES_CSID)
                .wait()
                .expect("Get phase failed")
                .unwrap()
                .to_string(),
            Phase::Public.to_string(),
            "get phase for the existing changeset"
        );

        assert_eq!(
            phases
                .get(ctx.clone(), repo.clone(), TWOS_CSID)
                .wait()
                .expect("Get phase failed")
                .is_some(),
            false,
            "get phase for non existing changeset"
        );
    }

    #[test]
    fn add_get_phase_test() {
        async_unit::tokio_unit_test(|| {
            add_get_phase(SqlPhases::with_sqlite_in_memory().unwrap());
        });
    }
}
