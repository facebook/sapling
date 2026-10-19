/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Perforce's data model, independent of how it is read.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Changelist {
    pub number: u64,
    pub user: Vec<u8>,
    /// Seconds since the epoch at which the changelist was submitted.
    pub time: u64,
    pub description: Vec<u8>,
    pub files: Vec<FileRevision>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRevision {
    pub depot_path: Vec<u8>,
    pub action: Action,
    pub file_type: String,
    pub revision: u32,
    /// `None` for `delete` and `move/delete`.
    pub size: Option<u64>,
}

/// The revision's current state, not necessarily the action submitted: Perforce rewrites
/// it to `purge` or `archive` afterwards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Add,
    Edit,
    Delete,
    Branch,
    Integrate,
    MoveAdd,
    MoveDelete,
    Import,
    Purge,
    Archive,
}
