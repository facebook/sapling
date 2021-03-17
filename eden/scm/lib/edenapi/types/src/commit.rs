/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde_derive::{Deserialize, Serialize};

use dag_types::Location;
use types::hgid::HgId;

/// Given a graph location, return `count` hashes following first parent links.
///
/// Example:
/// 0 - a - b - c
/// In this example our initial commit is `0`, then we have `a` the first commit, `b` second,
/// `c` third.
/// {
///   location: {
///     descendant: c,
///     distance: 1,
///   }
///   count: 2,
/// }
/// => [b, a]
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitLocationToHashRequest {
    pub location: Location<HgId>,
    pub count: u64,
}

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitLocationToHashResponse {
    pub location: Location<HgId>,
    pub count: u64,
    pub hgids: Vec<HgId>,
}

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitLocationToHashRequestBatch {
    pub requests: Vec<CommitLocationToHashRequest>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitLocationToHashRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitLocationToHashRequest {
            location: Arbitrary::arbitrary(g),
            count: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitLocationToHashResponse {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitLocationToHashResponse {
            location: Arbitrary::arbitrary(g),
            count: Arbitrary::arbitrary(g),
            hgids: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitLocationToHashRequestBatch {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitLocationToHashRequestBatch {
            requests: Arbitrary::arbitrary(g),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitHashToLocationRequestBatch {
    pub client_head: HgId,
    pub hgids: Vec<HgId>,
}

#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitHashToLocationResponse {
    pub hgid: HgId,
    pub location: Location<HgId>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitHashToLocationRequestBatch {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitHashToLocationRequestBatch {
            client_head: Arbitrary::arbitrary(g),
            hgids: Arbitrary::arbitrary(g),
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitHashToLocationResponse {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitHashToLocationResponse {
            hgid: Arbitrary::arbitrary(g),
            location: Arbitrary::arbitrary(g),
        }
    }
}

/// The list of Mercurial commit identifiers for which we want the commit data to be returned.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitRevlogDataRequest {
    pub hgids: Vec<HgId>,
}

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for CommitRevlogDataRequest {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        CommitRevlogDataRequest {
            hgids: Arbitrary::arbitrary(g),
        }
    }
}

/// A mercurial commit entry as it was serialized in the revlog.
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct CommitRevlogData {
    #[serde(with = "types::serde_with::hgid::bytes")]
    pub hgid: HgId,
    pub revlog_data: Bytes,
}

impl CommitRevlogData {
    pub fn new(hgid: HgId, revlog_data: Bytes) -> Self {
        Self { hgid, revlog_data }
    }
}
