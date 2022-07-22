/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::future::Future;

use serde::Deserialize;
use serde::Serialize;

/// The position of one node in the graph relative their known descendant.
/// This structure is relevant when the IdMap is lazy. Assuming that all parents of merges
/// and all heads are known then any node can be represented as their first `descendant`
/// and the distance to that descendant.
///
/// Example:
/// 0 - a - b - c
/// In this example our initial commit is `0`, then we have `a` the first commit, `b` second,
/// `c` third.
/// {
///   descendant: c,
///   distance: 1,
/// }
/// => [b]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct Location<Name> {
    pub descendant: Name,
    pub distance: u64,
}

impl<Name> Location<Name> {
    pub fn new(descendant: Name, distance: u64) -> Self {
        Self {
            descendant,
            distance,
        }
    }

    // What's up with all these map-like functions?
    // The most common operation for the location is to transform it between various
    // types of graphs, crate::Id, HgId, HgChangesetId, ChangesetId etc.
    // Had some fun adding with these functions. "Experimenting".

    pub fn map_descendant<T, F>(self, f: F) -> Location<T>
    where
        F: FnOnce(Name) -> T,
    {
        let new_name = f(self.descendant);
        Location::new(new_name, self.distance)
    }

    pub fn try_map_descendant<T, E, F>(self, f: F) -> Result<Location<T>, E>
    where
        F: FnOnce(Name) -> Result<T, E>,
    {
        let new_name = f(self.descendant)?;
        Ok(Location::new(new_name, self.distance))
    }

    pub async fn then_descendant<T, Fut, F>(self, f: F) -> Location<T>
    where
        F: FnOnce(Name) -> Fut,
        Fut: Future<Output = T>,
    {
        let new_name = f(self.descendant).await;
        Location::new(new_name, self.distance)
    }

    pub async fn and_then_descendant<T, E, Fut, F>(self, f: F) -> Result<Location<T>, E>
    where
        F: FnOnce(Name) -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        let new_name = f(self.descendant).await?;
        Ok(Location::new(new_name, self.distance))
    }

    pub fn with_descendant<T>(self, descendant: T) -> Location<T> {
        Location::new(descendant, self.distance)
    }
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Gen;

#[cfg(any(test, feature = "for-tests"))]
impl<Name> Arbitrary for Location<Name>
where
    Name: Arbitrary,
{
    fn arbitrary(g: &mut Gen) -> Self {
        Location {
            descendant: Name::arbitrary(g),
            distance: u64::arbitrary(g),
        }
    }
}
