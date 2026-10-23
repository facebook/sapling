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

/// What `p4 info` reports about the server, enough to tell one server from another.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerInfo {
    /// `None` until set with `p4 serverid`.
    pub server_id: Option<String>,
    /// `unknown` for an `rsh:` server.
    pub address: String,
    pub version: String,
    pub case_handling: CaseHandling,
    pub unicode: bool,
    /// For example `standard` or `forwarding-standby`.
    pub services: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseHandling {
    Sensitive,
    Insensitive,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Depot {
    /// Text, not bytes: callers pass `//{name}/...` to `submitted_changes`.
    pub name: String,
    pub depot_type: DepotType,
    pub map: Vec<u8>,
    /// The number of slashes in a stream's name after the depot name. Stream depots only.
    pub stream_depth: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DepotType {
    Local,
    Stream,
    Remote,
    Spec,
    Archive,
    Unload,
    Tangent,
    Graph,
    /// Any other type: one added by a newer server, or `extension` and `trait`, which only
    /// the server's hidden depots have.
    Other(String),
}
