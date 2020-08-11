/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use serde_derive::{Deserialize, Serialize};

use types::hgid::HgId;

/// Given the position of some node(s) in the commit graph, return the corresponding Mercurial
/// hashes of those nodes. The position is determined by a common known commit, identified by
/// it's Mercurial commit id, and the distance to reach our node(s) by repeatedly following
/// the first parent. The response is to return a set of addiacent nodes by following the first
/// parent thereafter.
///
/// Example:
/// 0 - a - b - c
/// In this example our initial commit is `0`, then we have `a` the first commit, `b` second,
/// `c` third.
/// {
///   known_descendant: c,
///   distance_to_descendant: 1,
///   count: 2,
/// }
/// => [b, a]
///
/// Notes.
///  * We expect the default or master bookmark to be a known commit.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct Location {
    pub known_descendant: HgId,
    pub distance_to_descendant: u64,
    pub count: u64,
}

/// A LocationToHashRequest consists of a set of locations that we want to retrieve the hashe for.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct LocationToHashRequest {
    pub locations: Vec<Location>,
}

/// Given a Location we want to return the hash for the commit that it points to in the graph.
/// LocationToHash groups together the Location and the commit hash for easy response construction.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct LocationToHash {
    pub location: Location,
    pub hgid: HgId,
}

impl Location {
    pub fn new(known_descendant: HgId, distance_to_descendant: u64, count: u64) -> Self {
        Self {
            known_descendant,
            distance_to_descendant,
            count,
        }
    }
}

impl LocationToHash {
    pub fn new(location: Location, hgid: HgId) -> Self {
        Self { location, hgid }
    }
}

/// The list of Mercurial commit identifiers for which we want the commit data to be returned.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitRevlogDataRequest {
    pub hgids: Vec<HgId>,
}

/// A mercurial commit entry as it was serialized in the revlog.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitRevlogData {
    pub hgid: HgId,
    pub revlog_data: Bytes,
}

impl CommitRevlogData {
    pub fn new(hgid: HgId, revlog_data: Bytes) -> Self {
        Self { hgid, revlog_data }
    }
}
