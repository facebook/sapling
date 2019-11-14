/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::iter::FromIterator;

use serde_derive::{Deserialize, Serialize};

use crate::hgid::{HgId, NULL_ID};

/// Enum representing a Mercurial hgid's parents.
///
/// A hgid may have zero, one, or two parents (referred to as p1 and p2 respectively).
/// Ordinarily, a non-existent parent is denoted by a null hash, consisting of all zeros.
/// A null p1 implies a null p2, so it is invalid for a hgid to have a p2 without a p1.
///
/// In Rust, these restrictions can be enforced with an enum that makes invalid
/// states unrepresentable.
#[derive(
    Clone,
    Debug,
    Eq,
    Hash,
    Ord,
    PartialEq,
    PartialOrd,
    Serialize,
    Deserialize
)]
#[serde(untagged)]
pub enum Parents {
    None,
    One(HgId),
    Two(HgId, HgId),
}

impl Parents {
    /// Construct a new Parents from two potentially null HgId hashes.
    /// This function will panic if an invalid combination of Nodes is given --
    /// namely, if p1 is null but p2 is not null.
    pub fn new(p1: HgId, p2: HgId) -> Self {
        match (p1, p2) {
            (NULL_ID, NULL_ID) => Parents::None,
            (p1, NULL_ID) => Parents::One(p1),
            (NULL_ID, _) => panic!("invalid parents: non-null p2 with null p1"),
            (p1, p2) => Parents::Two(p1, p2),
        }
    }

    /// Convert this Parents into a tuple representation, with non-existent
    /// parents represented by NULL_ID.
    pub fn into_nodes(self) -> (HgId, HgId) {
        match self {
            Parents::None => (NULL_ID, NULL_ID),
            Parents::One(p1) => (p1, NULL_ID),
            Parents::Two(p1, p2) => (p1, p2),
        }
    }

    pub fn p1(&self) -> Option<&HgId> {
        match self {
            Parents::None => None,
            Parents::One(ref p1) => Some(p1),
            Parents::Two(ref p1, _) => Some(p1),
        }
    }

    pub fn p2(&self) -> Option<&HgId> {
        match self {
            Parents::None | Parents::One(_) => None,
            Parents::Two(_, ref p2) => Some(p2),
        }
    }
}

impl Default for Parents {
    fn default() -> Self {
        Parents::None
    }
}

impl FromIterator<HgId> for Parents {
    fn from_iter<I: IntoIterator<Item = HgId>>(iter: I) -> Self {
        let mut iter = iter.into_iter();
        let p1 = iter.next().unwrap_or(NULL_ID);
        let p2 = iter.next().unwrap_or(NULL_ID);
        Parents::new(p1, p2)
    }
}

impl IntoIterator for Parents {
    type IntoIter = ParentIter;
    type Item = HgId;

    fn into_iter(self) -> ParentIter {
        ParentIter(self)
    }
}

impl IntoIterator for &Parents {
    type IntoIter = ParentIter;
    type Item = HgId;

    fn into_iter(self) -> ParentIter {
        ParentIter(self.clone())
    }
}

#[derive(Debug)]
pub struct ParentIter(Parents);

impl Iterator for ParentIter {
    type Item = HgId;

    fn next(&mut self) -> Option<Self::Item> {
        match self.0 {
            Parents::None => None,
            Parents::One(p1) => {
                self.0 = Parents::None;
                Some(p1)
            }
            Parents::Two(p1, p2) => {
                self.0 = Parents::One(p2);
                Some(p1)
            }
        }
    }
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for Parents {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        match g.next_u64() % 3 {
            0 => Parents::None,
            1 => Parents::One(HgId::arbitrary(g)),
            2 => Parents::Two(HgId::arbitrary(g), HgId::arbitrary(g)),
            _ => unreachable!(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::{self, json};

    #[test]
    fn from_iter() {
        let none = Parents::from_iter(vec![NULL_ID, NULL_ID]);
        assert_eq!(none, Parents::None);

        let p1 = HgId::from_byte_array([0xAA; 20]);
        let one = Parents::from_iter(vec![p1, NULL_ID]);
        assert_eq!(one, Parents::One(p1));

        let p2 = HgId::from_byte_array([0xBB; 20]);
        let two = Parents::from_iter(vec![p1, p2]);
        assert_eq!(two, Parents::Two(p1, p2));
    }

    #[test]
    #[should_panic]
    fn from_iter_invalid() {
        let p2 = HgId::from_byte_array([0xAA; 20]);
        let _ = Parents::from_iter(vec![NULL_ID, p2]);
    }

    #[test]
    fn into_iter() {
        let parents = Parents::None;
        let none = parents.into_iter().collect::<Vec<_>>();
        assert_eq!(none, Vec::new());

        let p1 = HgId::from_byte_array([0xAA; 20]);
        let parents = Parents::One(p1);
        let one = parents.into_iter().collect::<Vec<_>>();
        assert_eq!(one, vec![p1]);

        let p2 = HgId::from_byte_array([0xBB; 20]);
        let parents = Parents::Two(p1, p2);
        let two = parents.into_iter().collect::<Vec<_>>();
        assert_eq!(two, vec![p1, p2]);
    }

    #[test]
    fn untagged_serialization() {
        let parents = Parents::None;
        let none = serde_json::to_value(&parents).unwrap();
        assert_eq!(none, json!(null));

        let p1 = HgId::from_byte_array([0x1; 20]);
        let parents = Parents::One(p1);
        let one = serde_json::to_value(&parents).unwrap();
        let expected = json!([1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]);
        assert_eq!(one, expected);

        let p2 = HgId::from_byte_array([0x2; 20]);
        let parents = Parents::Two(p1, p2);
        let two = serde_json::to_value(&parents).unwrap();
        let p1_json = json!([1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1]);
        let p2_json = json!([2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2]);
        assert_eq!(two, json!([p1_json, p2_json]));
    }

    #[test]
    fn serialized_size_cbor() {
        let parents = Parents::None;
        let none = serde_cbor::to_vec(&parents).unwrap();
        assert_eq!(none.len(), 1);

        let p1 = HgId::from_byte_array([0x1; 20]);
        let parents = Parents::One(p1);
        let one = serde_cbor::to_vec(&parents).unwrap();
        assert_eq!(one.len(), 21);

        let p2 = HgId::from_byte_array([0x2; 20]);
        let parents = Parents::Two(p1, p2);
        let two = serde_cbor::to_vec(&parents).unwrap();
        assert_eq!(two.len(), 43);
    }
}
