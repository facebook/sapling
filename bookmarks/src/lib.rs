// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate ascii;
extern crate failure_ext as failure;
extern crate futures_ext;
extern crate mercurial_types;

use ascii::AsciiString;
use failure::{Error, Result};
use futures_ext::{BoxFuture, BoxStream};
use mercurial_types::{HgChangesetId, RepositoryId};

pub trait Bookmarks: Send + Sync + 'static {
    /// Returns Some(HgChangesetId) if bookmark exists, returns None if doesn't
    fn get(
        &self,
        name: &AsciiString,
        repoid: &RepositoryId,
    ) -> BoxFuture<Option<HgChangesetId>, Error>;

    /// Lists the bookmarks that match the prefix with bookmark's values.
    /// Empty prefix means list all of the available bookmarks
    /// TODO(stash): do we need to have a separate method list_all() to avoid accidentally
    /// listing all the bookmarks?
    fn list_by_prefix(
        &self,
        prefix: &AsciiString,
        repoid: &RepositoryId,
    ) -> BoxStream<(AsciiString, HgChangesetId), Error>;

    /// Creates a transaction that will be used for write operations.
    fn create_transaction(&self, repoid: &RepositoryId) -> Box<Transaction>;
}

pub trait Transaction: Send + Sync + 'static {
    /// Adds set() operation to the transaction set.
    /// Updates a bookmark's value. Bookmark should already exist and point to `old_cs`, otherwise
    /// committing the transaction will fail.
    fn update(
        &mut self,
        key: &AsciiString,
        new_cs: &HgChangesetId,
        old_cs: &HgChangesetId,
    ) -> Result<()>;

    /// Adds create() operation to the transaction set.
    /// Creates a bookmark. Bookmark should not already exist, otherwise committing the
    /// transaction will fail.
    fn create(&mut self, key: &AsciiString, new_cs: &HgChangesetId) -> Result<()>;

    /// Adds force_set() operation to the transaction set.
    /// Unconditionally sets the new value of the bookmark. Succeeds regardless of whether bookmark
    /// exists or not.
    fn force_set(&mut self, key: &AsciiString, new_cs: &HgChangesetId) -> Result<()>;

    /// Adds delete operation to the transaction set.
    /// Deletes bookmark only if it currently points to `old_cs`.
    fn delete(&mut self, key: &AsciiString, old_cs: &HgChangesetId) -> Result<()>;

    /// Adds force_delete operation to the transaction set.
    /// Deletes bookmark unconditionally.
    fn force_delete(&mut self, key: &AsciiString) -> Result<()>;

    /// Commits the transaction. Future succeeds if transaction has been
    /// successful, or errors if transaction has failed. Transaction may fail because of the
    /// infra error or logical error i.e. non-existent bookmark was deleted.
    fn commit(&self) -> BoxFuture<(), Error>;
}
