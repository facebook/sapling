// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate ascii;
#[macro_use]
extern crate failure_ext as failure;
extern crate futures_ext;
extern crate mercurial_types;
extern crate mononoke_types;
extern crate sql;

use std::fmt;

use ascii::AsciiString;
use failure::{Error, Result};
use futures_ext::{BoxFuture, BoxStream};
use mercurial_types::RepositoryId;
use mononoke_types::ChangesetId;
use sql::mysql_async::{FromValueError, Value, prelude::{ConvIr, FromValue}};

type FromValueResult<T> = ::std::result::Result<T, FromValueError>;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Bookmark {
    bookmark: AsciiString,
}

impl fmt::Display for Bookmark {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.bookmark)
    }
}

impl Bookmark {
    pub fn new<B: AsRef<str>>(bookmark: B) -> Result<Self> {
        Ok(Self {
            bookmark: AsciiString::from_ascii(bookmark.as_ref())
                .map_err(|bytes| format_err!("non-ascii bookmark name: {:?}", bytes))?,
        })
    }

    pub fn new_ascii(bookmark: AsciiString) -> Self {
        Self { bookmark }
    }

    pub fn to_ascii(&self) -> Result<AsciiString> {
        Ok(self.bookmark.clone())
    }

    pub fn to_string(&self) -> String {
        self.bookmark.clone().into()
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BookmarkPrefix {
    bookmark_prefix: AsciiString,
}

impl fmt::Display for BookmarkPrefix {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.bookmark_prefix)
    }
}

impl BookmarkPrefix {
    pub fn new<B: AsRef<str>>(bookmark_prefix: B) -> Result<Self> {
        Ok(Self {
            bookmark_prefix: AsciiString::from_ascii(bookmark_prefix.as_ref())
                .map_err(|bytes| format_err!("non-ascii bookmark prefix: {:?}", bytes))?,
        })
    }

    pub fn new_ascii(bookmark_prefix: AsciiString) -> Self {
        Self { bookmark_prefix }
    }

    pub fn empty() -> Self {
        Self {
            bookmark_prefix: AsciiString::default(),
        }
    }

    pub fn to_ascii(&self) -> Result<AsciiString> {
        Ok(self.bookmark_prefix.clone())
    }

    pub fn to_string(&self) -> String {
        self.bookmark_prefix.clone().into()
    }

    pub fn is_empty(&self) -> bool {
        self.bookmark_prefix.is_empty()
    }
}

pub trait Bookmarks: Send + Sync + 'static {
    /// Returns Some(ChangesetId) if bookmark exists, returns None if doesn't
    fn get(&self, name: &Bookmark, repoid: &RepositoryId) -> BoxFuture<Option<ChangesetId>, Error>;

    /// Lists the bookmarks that match the prefix with bookmark's values.
    /// Empty prefix means list all of the available bookmarks
    /// TODO(stash): do we need to have a separate method list_all() to avoid accidentally
    /// listing all the bookmarks?
    fn list_by_prefix(
        &self,
        prefix: &BookmarkPrefix,
        repoid: &RepositoryId,
    ) -> BoxStream<(Bookmark, ChangesetId), Error>;

    /// Creates a transaction that will be used for write operations.
    fn create_transaction(&self, repoid: &RepositoryId) -> Box<Transaction>;
}

pub trait Transaction: Send + Sync + 'static {
    /// Adds set() operation to the transaction set.
    /// Updates a bookmark's value. Bookmark should already exist and point to `old_cs`, otherwise
    /// committing the transaction will fail.
    fn update(&mut self, key: &Bookmark, new_cs: &ChangesetId, old_cs: &ChangesetId) -> Result<()>;

    /// Adds create() operation to the transaction set.
    /// Creates a bookmark. Bookmark should not already exist, otherwise committing the
    /// transaction will fail.
    fn create(&mut self, key: &Bookmark, new_cs: &ChangesetId) -> Result<()>;

    /// Adds force_set() operation to the transaction set.
    /// Unconditionally sets the new value of the bookmark. Succeeds regardless of whether bookmark
    /// exists or not.
    fn force_set(&mut self, key: &Bookmark, new_cs: &ChangesetId) -> Result<()>;

    /// Adds delete operation to the transaction set.
    /// Deletes bookmark only if it currently points to `old_cs`.
    fn delete(&mut self, key: &Bookmark, old_cs: &ChangesetId) -> Result<()>;

    /// Adds force_delete operation to the transaction set.
    /// Deletes bookmark unconditionally.
    fn force_delete(&mut self, key: &Bookmark) -> Result<()>;

    /// Commits the transaction. Future succeeds if transaction has been
    /// successful, or errors if transaction has failed. Logical failure is indicated by
    /// returning a successful `false` value; infrastructure failure is reported via an Error.
    fn commit(self: Box<Self>) -> BoxFuture<bool, Error>;
}

impl From<Bookmark> for Value {
    fn from(bookmark: Bookmark) -> Self {
        Value::Bytes(bookmark.bookmark.into())
    }
}

impl ConvIr<Bookmark> for Bookmark {
    fn new(v: Value) -> FromValueResult<Self> {
        match v {
            Value::Bytes(bytes) => AsciiString::from_ascii(bytes)
                .map_err(|err| FromValueError(Value::Bytes(err.into_source())))
                .map(Bookmark::new_ascii),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> Bookmark {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for Bookmark {
    type Intermediate = Bookmark;
}

impl From<BookmarkPrefix> for Value {
    fn from(bookmark_prefix: BookmarkPrefix) -> Self {
        Value::Bytes(bookmark_prefix.bookmark_prefix.into())
    }
}
