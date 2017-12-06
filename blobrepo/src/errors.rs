// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fmt;

pub use failure::Error;

use mercurial_types::{BlobHash, NodeHash};

#[derive(Debug)]
pub enum StateOpenError {
    Heads,
    Bookmarks,
    Blobstore,
    Linknodes,
}

impl fmt::Display for StateOpenError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use StateOpenError::*;

        match *self {
            Heads => write!(f, "heads"),
            Bookmarks => write!(f, "bookmarks"),
            Blobstore => write!(f, "blob store"),
            Linknodes => write!(f, "linknodes"),
        }
    }
}

pub type Result<T> = ::std::result::Result<T, Error>;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Error while opening state for {}", _0)] StateOpen(StateOpenError),
    #[fail(display = "Changeset id {} is missing", _0)] ChangesetMissing(NodeHash),
    #[fail(display = "Manifest id {} is missing", _0)] ManifestMissing(NodeHash),
    #[fail(display = "Node id {} is missing", _0)] NodeMissing(NodeHash),
    #[fail(display = "Content missing nodeid {} (blob hash {:?})", _0, _1)]
    ContentMissing(NodeHash, BlobHash),
}
