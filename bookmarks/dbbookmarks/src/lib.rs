// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
#![feature(never_type)]

extern crate ascii;
extern crate bookmarks;
#[macro_use]
extern crate diesel;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures;
#[macro_use]
extern crate futures_ext;
extern crate mercurial_types;
#[cfg(test)]
extern crate mercurial_types_mocks;
extern crate storage_types;

mod schema;
mod models;

use ascii::AsciiString;
use bookmarks::{Bookmarks, Transaction};
use diesel::{delete, insert_into, replace_into, update, SqliteConnection};
use diesel::connection::SimpleConnection;
use diesel::prelude::*;
use failure::{Error, Result};
use futures::{future, stream, Future, IntoFuture, Stream};
use futures_ext::{BoxFuture, BoxStream, FutureExt, StreamExt};

use mercurial_types::{DChangesetId, RepositoryId};
use std::collections::{HashMap, HashSet};
use std::result;
use std::sync::{Arc, Mutex, MutexGuard};

pub struct SqliteDbBookmarks {
    connection: Arc<Mutex<SqliteConnection>>,
}

impl SqliteDbBookmarks {
    /// Open a SQLite database. This is synchronous because the SQLite backend hits local
    /// disk or memory.
    pub fn open<P: AsRef<str>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let conn = SqliteConnection::establish(path)?;
        Ok(Self {
            connection: Arc::new(Mutex::new(conn)),
        })
    }

    /// Create a new SQLite database.
    pub fn create<P: AsRef<str>>(path: P) -> Result<Self> {
        let bookmarks = Self::open(path)?;

        let up_query = include_str!("../schemas/sqlite-bookmarks.sql");
        bookmarks
            .connection
            .lock()
            .expect("lock poisoned")
            .batch_execute(&up_query)?;

        Ok(bookmarks)
    }

    /// Create a new in-memory empty database. Great for tests.
    pub fn in_memory() -> Result<Self> {
        Self::create(":memory:")
    }

    fn get_conn(&self) -> result::Result<MutexGuard<SqliteConnection>, !> {
        Ok(self.connection.lock().expect("lock poisoned"))
    }
}

impl Bookmarks for SqliteDbBookmarks {
    fn get(
        &self,
        name: &AsciiString,
        repo_id: &RepositoryId,
    ) -> BoxFuture<Option<DChangesetId>, Error> {
        #[allow(unreachable_code, unreachable_patterns)] // sqlite can't fail
        let connection = try_boxfuture!(self.get_conn());

        let name = name.as_str().to_string();
        schema::bookmarks::table
            .filter(schema::bookmarks::repo_id.eq(repo_id))
            .filter(schema::bookmarks::name.eq(name))
            .select(schema::bookmarks::changeset_id)
            .first::<DChangesetId>(&*connection)
            .optional()
            .into_future()
            .from_err()
            .boxify()
    }

    fn list_by_prefix(
        &self,
        prefix: &AsciiString,
        repo_id: &RepositoryId,
    ) -> BoxStream<(AsciiString, DChangesetId), Error> {
        #[allow(unreachable_code, unreachable_patterns)] // sqlite can't fail
        let connection = match self.get_conn() {
            Ok(conn) => conn,
            Err(err) => {
                return stream::once(err).boxify();
            }
        };

        let prefix = prefix.as_str().to_string();
        let query = schema::bookmarks::table
            .filter(schema::bookmarks::repo_id.eq(repo_id))
            .filter(schema::bookmarks::name.like(format!("{}%", prefix)));

        query
            .get_results::<models::BookmarkRow>(&*connection)
            .into_future()
            .and_then(|bookmarks| {
                let bookmarks = bookmarks
                    .into_iter()
                    .map(|row| (row.name, row.changeset_id));
                Ok(stream::iter_ok(bookmarks).boxify())
            })
            .from_err()
            .flatten_stream()
            .and_then(|entry| Ok((AsciiString::from_ascii(entry.0)?, entry.1)))
            .boxify()
    }

    fn create_transaction(&self, repoid: &RepositoryId) -> Box<Transaction> {
        Box::new(SqliteBookmarksTransaction::new(
            self.connection.clone(),
            repoid,
        ))
    }
}

struct BookmarkSetData {
    new_cs: DChangesetId,
    old_cs: DChangesetId,
}

struct SqliteBookmarksTransaction {
    connection: Arc<Mutex<SqliteConnection>>,
    force_sets: HashMap<AsciiString, DChangesetId>,
    creates: HashMap<AsciiString, DChangesetId>,
    sets: HashMap<AsciiString, BookmarkSetData>,
    force_deletes: HashSet<AsciiString>,
    deletes: HashMap<AsciiString, DChangesetId>,
    repo_id: RepositoryId,
}

impl SqliteBookmarksTransaction {
    fn new(connection: Arc<Mutex<SqliteConnection>>, repo_id: &RepositoryId) -> Self {
        Self {
            connection: connection,
            force_sets: HashMap::new(),
            creates: HashMap::new(),
            sets: HashMap::new(),
            force_deletes: HashSet::new(),
            deletes: HashMap::new(),
            repo_id: *repo_id,
        }
    }

    fn check_if_bookmark_already_used(&self, key: &AsciiString) -> Result<()> {
        if self.creates.contains_key(key) || self.force_sets.contains_key(key)
            || self.sets.contains_key(key) || self.force_deletes.contains(key)
            || self.deletes.contains_key(key)
        {
            bail_msg!("{} bookmark was already used", key);
        }
        Ok(())
    }
}

impl Transaction for SqliteBookmarksTransaction {
    fn update(
        &mut self,
        key: &AsciiString,
        new_cs: &DChangesetId,
        old_cs: &DChangesetId,
    ) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.sets.insert(
            key.clone(),
            BookmarkSetData {
                new_cs: *new_cs,
                old_cs: *old_cs,
            },
        );
        Ok(())
    }

    fn create(&mut self, key: &AsciiString, new_cs: &DChangesetId) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.creates.insert(key.clone(), *new_cs);
        Ok(())
    }

    fn force_set(&mut self, key: &AsciiString, new_cs: &DChangesetId) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.force_sets.insert(key.clone(), *new_cs);
        Ok(())
    }

    fn delete(&mut self, key: &AsciiString, old_cs: &DChangesetId) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.deletes.insert(key.clone(), *old_cs);
        Ok(())
    }

    fn force_delete(&mut self, key: &AsciiString) -> Result<()> {
        self.check_if_bookmark_already_used(key)?;
        self.force_deletes.insert(key.clone());
        Ok(())
    }

    fn commit(&self) -> BoxFuture<(), Error> {
        let connection = self.connection.lock().expect("lock poisoned");
        let txnres = connection.transaction::<_, Error, _>(|| {
            replace_into(schema::bookmarks::table)
                .values(&create_bookmarks_rows(self.repo_id, &self.force_sets))
                .execute(&*connection)?;

            insert_into(schema::bookmarks::table)
                .values(&create_bookmarks_rows(self.repo_id, &self.creates))
                .execute(&*connection)?;

            for (key, &BookmarkSetData { new_cs, old_cs }) in self.sets.iter() {
                let key = key.as_str().to_string();
                let num_affected_rows = update(
                    schema::bookmarks::table
                        .filter(schema::bookmarks::name.eq(key.clone()))
                        .filter(schema::bookmarks::changeset_id.eq(old_cs)),
                ).set(schema::bookmarks::changeset_id.eq(new_cs))
                    .execute(&*connection)?;
                if num_affected_rows != 1 {
                    bail_msg!("cannot update bookmark {}", key);
                }
            }

            for key in self.force_deletes.iter() {
                let key = key.as_str().to_string();
                delete(schema::bookmarks::table.filter(schema::bookmarks::name.eq(key)))
                    .execute(&*connection)?;
            }

            for (key, old_cs) in self.deletes.iter() {
                let key = key.as_str().to_string();
                let num_deleted_rows = delete(
                    schema::bookmarks::table
                        .filter(schema::bookmarks::name.eq(key.clone()))
                        .filter(schema::bookmarks::changeset_id.eq(old_cs)),
                ).execute(&*connection)?;
                if num_deleted_rows != 1 {
                    bail_msg!("cannot delete bookmark {}", key);
                }
            }
            Ok(())
        });
        future::result(txnres).from_err().boxify()
    }
}

fn create_bookmarks_rows(
    repo_id: RepositoryId,
    map: &HashMap<AsciiString, DChangesetId>,
) -> Vec<models::BookmarkRow> {
    map.iter()
        .map(|(name, changeset_id)| models::BookmarkRow {
            repo_id,
            name: name.as_str().to_string(),
            changeset_id: *changeset_id,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mercurial_types_mocks::nodehash::{ONES_CSID, TWOS_CSID};
    use mercurial_types_mocks::repo::REPO_ZERO;

    #[test]
    fn test_simple_unconditional_set_get() {
        let bookmarks = SqliteDbBookmarks::in_memory().unwrap();
        let name_correct = AsciiString::from_ascii("book".to_string()).unwrap();
        let name_incorrect = AsciiString::from_ascii("book2".to_string()).unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.force_set(&name_correct, &ONES_CSID).unwrap();
        txn.commit().wait().unwrap();

        assert_eq!(
            bookmarks.get(&name_correct, &REPO_ZERO).wait().unwrap(),
            Some(ONES_CSID)
        );
        assert_eq!(
            bookmarks.get(&name_incorrect, &REPO_ZERO).wait().unwrap(),
            None
        );
    }

    #[test]
    fn test_multi_unconditional_set_get() {
        let bookmarks = SqliteDbBookmarks::in_memory().unwrap();
        let name_1 = AsciiString::from_ascii("book".to_string()).unwrap();
        let name_2 = AsciiString::from_ascii("book2".to_string()).unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.force_set(&name_1, &ONES_CSID).unwrap();
        txn.force_set(&name_2, &TWOS_CSID).unwrap();
        txn.commit().wait().unwrap();

        assert_eq!(
            bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap(),
            Some(ONES_CSID)
        );
        assert_eq!(
            bookmarks.get(&name_2, &REPO_ZERO).wait().unwrap(),
            Some(TWOS_CSID)
        );
    }

    #[test]
    fn test_unconditional_set_same_bookmark() {
        let bookmarks = SqliteDbBookmarks::in_memory().unwrap();
        let name_1 = AsciiString::from_ascii("book".to_string()).unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.force_set(&name_1, &ONES_CSID).unwrap();
        txn.commit().wait().unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.force_set(&name_1, &ONES_CSID).unwrap();
        txn.commit().wait().unwrap();

        assert_eq!(
            bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap(),
            Some(ONES_CSID)
        );
    }

    #[test]
    fn test_simple_create() {
        let bookmarks = SqliteDbBookmarks::in_memory().unwrap();
        let name_1 = AsciiString::from_ascii("book".to_string()).unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.create(&name_1, &ONES_CSID).unwrap();
        txn.commit().wait().unwrap();

        assert_eq!(
            bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap(),
            Some(ONES_CSID)
        );
    }

    #[test]
    fn test_create_already_existing() {
        let bookmarks = SqliteDbBookmarks::in_memory().unwrap();
        let name_1 = AsciiString::from_ascii("book".to_string()).unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.create(&name_1, &ONES_CSID).unwrap();
        txn.commit().wait().unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.create(&name_1, &ONES_CSID).unwrap();
        assert!(txn.commit().wait().is_err());
    }

    #[test]
    fn test_create_change_same_bookmark() {
        let bookmarks = SqliteDbBookmarks::in_memory().unwrap();
        let name_1 = AsciiString::from_ascii("book".to_string()).unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.create(&name_1, &ONES_CSID).unwrap();
        assert!(txn.force_set(&name_1, &ONES_CSID).is_err());

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.force_set(&name_1, &ONES_CSID).unwrap();
        assert!(txn.create(&name_1, &ONES_CSID).is_err());

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.force_set(&name_1, &ONES_CSID).unwrap();
        assert!(txn.update(&name_1, &TWOS_CSID, &ONES_CSID).is_err());

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.update(&name_1, &TWOS_CSID, &ONES_CSID).unwrap();
        assert!(txn.force_set(&name_1, &ONES_CSID).is_err());

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.update(&name_1, &TWOS_CSID, &ONES_CSID).unwrap();
        assert!(txn.force_delete(&name_1).is_err());

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.force_delete(&name_1).unwrap();
        assert!(txn.update(&name_1, &TWOS_CSID, &ONES_CSID).is_err());

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.delete(&name_1, &ONES_CSID).unwrap();
        assert!(txn.update(&name_1, &TWOS_CSID, &ONES_CSID).is_err());

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.update(&name_1, &TWOS_CSID, &ONES_CSID).unwrap();
        assert!(txn.delete(&name_1, &ONES_CSID).is_err());
    }

    #[test]
    fn test_simple_update_bookmark() {
        let bookmarks = SqliteDbBookmarks::in_memory().unwrap();
        let name_1 = AsciiString::from_ascii("book".to_string()).unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.create(&name_1, &ONES_CSID).unwrap();
        txn.commit().wait().unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.update(&name_1, &TWOS_CSID, &ONES_CSID).unwrap();
        txn.commit().wait().unwrap();

        assert_eq!(
            bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap(),
            Some(TWOS_CSID)
        );
    }

    #[test]
    fn test_update_non_existent_bookmark() {
        let bookmarks = SqliteDbBookmarks::in_memory().unwrap();
        let name_1 = AsciiString::from_ascii("book".to_string()).unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.update(&name_1, &TWOS_CSID, &ONES_CSID).unwrap();
        assert!(txn.commit().wait().is_err());
    }

    #[test]
    fn test_update_existing_bookmark_with_incorrect_commit() {
        let bookmarks = SqliteDbBookmarks::in_memory().unwrap();
        let name_1 = AsciiString::from_ascii("book".to_string()).unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.create(&name_1, &ONES_CSID).unwrap();
        txn.commit().wait().unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.update(&name_1, &ONES_CSID, &TWOS_CSID).unwrap();
        assert!(txn.commit().wait().is_err());
    }

    #[test]
    fn test_force_delete() {
        let bookmarks = SqliteDbBookmarks::in_memory().unwrap();
        let name_1 = AsciiString::from_ascii("book".to_string()).unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.force_delete(&name_1).unwrap();
        txn.commit().wait().unwrap();

        assert_eq!(bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap(), None);

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.create(&name_1, &ONES_CSID).unwrap();
        txn.commit().wait().unwrap();
        assert!(bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap().is_some());

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.force_delete(&name_1).unwrap();
        txn.commit().wait().unwrap();

        assert_eq!(bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap(), None);
    }

    #[test]
    fn test_delete() {
        let bookmarks = SqliteDbBookmarks::in_memory().unwrap();
        let name_1 = AsciiString::from_ascii("book".to_string()).unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.delete(&name_1, &ONES_CSID).unwrap();
        assert!(txn.commit().wait().is_err());

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.create(&name_1, &ONES_CSID).unwrap();
        txn.commit().wait().unwrap();
        assert!(bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap().is_some());

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.delete(&name_1, &ONES_CSID).unwrap();
        txn.commit().wait().unwrap();
    }

    #[test]
    fn test_delete_incorrect_hash() {
        let bookmarks = SqliteDbBookmarks::in_memory().unwrap();
        let name_1 = AsciiString::from_ascii("book".to_string()).unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.create(&name_1, &ONES_CSID).unwrap();
        txn.commit().wait().unwrap();
        assert!(bookmarks.get(&name_1, &REPO_ZERO).wait().unwrap().is_some());

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.delete(&name_1, &TWOS_CSID).unwrap();
        assert!(txn.commit().wait().is_err());
    }

    #[test]
    fn test_list_by_prefix() {
        let bookmarks = SqliteDbBookmarks::in_memory().unwrap();
        let name_1 = AsciiString::from_ascii("book1".to_string()).unwrap();
        let name_2 = AsciiString::from_ascii("book2".to_string()).unwrap();

        let mut txn = bookmarks.create_transaction(&REPO_ZERO);
        txn.create(&name_1, &ONES_CSID).unwrap();
        txn.create(&name_2, &ONES_CSID).unwrap();
        txn.commit().wait().unwrap();

        let prefix = AsciiString::from_ascii("book".to_string()).unwrap();
        assert_eq!(
            bookmarks
                .list_by_prefix(&prefix, &REPO_ZERO)
                .collect()
                .wait()
                .unwrap(),
            vec![(name_1.clone(), ONES_CSID), (name_2.clone(), ONES_CSID)]
        );

        assert_eq!(
            bookmarks
                .list_by_prefix(&name_1, &REPO_ZERO)
                .collect()
                .wait()
                .unwrap(),
            vec![(name_1.clone(), ONES_CSID)]
        );

        assert_eq!(
            bookmarks
                .list_by_prefix(&name_2, &REPO_ZERO)
                .collect()
                .wait()
                .unwrap(),
            vec![(name_2, ONES_CSID)]
        );
    }
}
