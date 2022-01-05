/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
use serde::Deserialize;
use serde::Serialize;

/// Generic container for a bunch of uniform objects. This is primarily intended for requests and
/// responses which can be difficult to evolve when the top level object is an array.
/// For cases where evolution is required we would probably replace the Batch wrapper with a
/// specialized type. For example, starting from `Batch<MyRequest>` we would change to
/// struct MyRequestBatch {
///   pub batch: Vec<MyRequest>,
///   pub evolution: Evolution,
/// }
#[derive(Clone, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[derive(Serialize, Deserialize)]
pub struct Batch<T> {
    pub batch: Vec<T>,
}

#[cfg(any(test, feature = "for-tests"))]
impl<T> Arbitrary for Batch<T>
where
    T: Arbitrary,
{
    fn arbitrary(g: &mut quickcheck::Gen) -> Self {
        Batch {
            batch: Arbitrary::arbitrary(g),
        }
    }
}
