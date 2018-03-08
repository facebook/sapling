// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Definition of errors used in this crate by the error_chain crate

use std::collections::VecDeque;

use mercurial_types::MPathElement;

pub use failure::{Error, Result, ResultExt};

/// Possible VFS errors
#[derive(Debug, Fail)]
pub enum ErrorKind {
    /// Inserting a leaf into the tree in an invalid position. Most commonly this can happen
    /// when inserting a leaf would change an existing leaf into a node
    #[fail(display = "TreeInsert: {}", _0)]
    TreeInsert(String),
    /// Tried to walk on a path that does not exists. Returns the remainder of walk.
    #[fail(display = "PathDoesNotExist: {}", _0)]
    PathDoesNotExist(String, VecDeque<MPathElement>),
    /// TODO(luk, T20453159) This is a temporary error, will be removed once all the
    /// functionalities of this library are finished
    #[fail(display = "Not implemented yet: {}", _0)]
    NotImplemented(String),
    /// Reached maximum number of steps on the walk. Most commonly this happens when a symlink
    /// that leads into an infinite loop when resolved. Returns the remainder of walk.
    #[fail(display = "maximum number of steps during a walk on Vfs was reached: {}", _0)]
    MaximumStepReached(String, VecDeque<MPathElement>),
    /// One of the paths in entries listed by manifest contained an invalid (f.e. empty) Path
    #[fail(display = "manifest contained an invalid path: {}", _0)]
    ManifestInvalidPath(String),
}
