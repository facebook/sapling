/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Perforce's data model, independent of how it is read.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Changelist {
    pub number: u64,
    pub user: String,
    /// Seconds since the epoch at which the changelist was submitted.
    pub time: u64,
    pub description: String,
    pub files: Vec<FileRevision>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRevision {
    /// Strict UTF-8 is exactly the bytes p4 sent, so passing it back to p4 is lossless.
    pub depot_path: String,
    pub action: Action,
    pub file_type: String,
    pub revision: u32,
    /// `None` for `delete` and `move/delete`.
    pub size: Option<u64>,
    /// Of the content `p4 print` sends. Never without `size`, but may be missing with it.
    pub digest: Option<Md5Digest>,
}

/// Shown as uppercase hex, as p4 reports it.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Md5Digest([u8; 16]);

impl Md5Digest {
    pub fn new(digest: [u8; 16]) -> Self {
        Self(digest)
    }

    /// 32 hex digits in either case.
    pub fn from_hex(hex: &[u8]) -> Option<Self> {
        if hex.len() != 32 {
            return None;
        }
        let nibble = |digit: u8| char::from(digit).to_digit(16);
        let mut digest = [0; 16];
        for (byte, [high, low]) in digest.iter_mut().zip(hex.as_chunks::<2>().0) {
            *byte = u8::try_from(nibble(*high)? << 4 | nibble(*low)?).ok()?;
        }
        Some(Self(digest))
    }
}

impl fmt::Display for Md5Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|byte| write!(f, "{byte:02X}"))
    }
}

impl fmt::Debug for Md5Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Md5Digest({self})")
    }
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

impl Action {
    /// Purged and archived revisions keep their size and digest, but p4 prints nothing.
    pub fn has_content(self) -> bool {
        match self {
            Action::Add
            | Action::Edit
            | Action::Branch
            | Action::Integrate
            | Action::MoveAdd
            | Action::Import => true,
            Action::Delete | Action::MoveDelete | Action::Purge | Action::Archive => false,
        }
    }
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
    pub name: String,
    pub depot_type: DepotType,
    pub map: String,
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

#[cfg(test)]
mod tests;
