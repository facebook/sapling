/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::ops::{Add, Mul};
use std::str::FromStr;
use std::u32;

/// Index into a `RevLog`
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct RevIdx(u32);

// Implement `RevIdx`s methods
impl RevIdx {
    /// Return index for first entry
    pub fn zero() -> Self {
        RevIdx(0)
    }

    /// Return successor index
    pub fn succ(self) -> Self {
        RevIdx(self.0 + 1)
    }

    /// Return previous index
    ///
    /// Panics if index is zero.
    pub fn pred(self) -> Self {
        assert!(self.0 > 0);
        RevIdx(self.0 - 1)
    }

    /// Return iterator for a range from index to `lim`.
    pub fn range_to(&self, lim: Self) -> RevIdxRange {
        RevIdxRange(self.0, lim.0)
    }

    /// Return an open ended iterator from index.
    pub fn range(&self) -> RevIdxRange {
        RevIdxRange(self.0, u32::MAX)
    }

    pub fn as_u32(&self) -> u32 {
        self.0
    }
}

// Construct a `RevIdx` from a `u32`
impl From<u32> for RevIdx {
    fn from(v: u32) -> Self {
        RevIdx(v)
    }
}

// Construct a `RevIdx` from a `usize`
// Panics if the usize is larger than u32::MAX
impl From<usize> for RevIdx {
    fn from(v: usize) -> Self {
        assert!(v <= u32::MAX as usize);
        RevIdx(v as u32)
    }
}

// Construct a `RevIdx` from a string (which may fail)
impl FromStr for RevIdx {
    type Err = <u32 as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        u32::from_str(s).map(RevIdx)
    }
}

// Multiply operator for RevIdx * usize -> usize
// Used for constructing a byte offset for an index
impl Mul<usize> for RevIdx {
    type Output = usize;

    fn mul(self, other: usize) -> Self::Output {
        self.0 as usize * other
    }
}

// RevIdx + usize -> RevIdx
impl Add<usize> for RevIdx {
    type Output = RevIdx;

    fn add(self, other: usize) -> Self::Output {
        RevIdx((self.0 as usize + other) as u32)
    }
}

// Convert a `RevIdx` into an open-ended iterator of RevIdx values
// starting at RevIdx's value. ie, RevIdx(2).into_iter() => RevIdx(2), RevIdx(3), ...
impl<'a> IntoIterator for &'a RevIdx {
    type Item = RevIdx;
    type IntoIter = RevIdxRange;

    fn into_iter(self) -> Self::IntoIter {
        self.range()
    }
}

/// An open-ended or bounded iterator over a range of RevIdx
#[derive(Copy, Clone, Debug)]
pub struct RevIdxRange(u32, u32);

impl Iterator for RevIdxRange {
    type Item = RevIdx;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0 < self.1 {
            let ret = RevIdx(self.0);
            self.0 += 1;
            Some(ret)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn zero() {
        assert_eq!(RevIdx::zero(), RevIdx(0))
    }

    #[test]
    fn succ() {
        assert_eq!(RevIdx::zero().succ(), RevIdx(1));
        assert_eq!(RevIdx::zero().succ().succ(), RevIdx(2));
    }

    #[test]
    fn pred() {
        assert_eq!(RevIdx(10).pred(), RevIdx(9));
    }

    #[test]
    #[should_panic]
    fn bad_pred() {
        println!("bad {:?}", RevIdx::zero().pred());
    }

    #[test]
    fn range_to() {
        let v: Vec<_> = RevIdx::zero().range_to(RevIdx(5)).collect();
        assert_eq!(
            v,
            vec![RevIdx(0), RevIdx(1), RevIdx(2), RevIdx(3), RevIdx(4)]
        );
    }

    #[test]
    fn iter() {
        let v: Vec<_> = RevIdx::zero().into_iter().take(5).collect();
        assert_eq!(
            v,
            vec![RevIdx(0), RevIdx(1), RevIdx(2), RevIdx(3), RevIdx(4)]
        )
    }

    #[test]
    fn fromstr() {
        let idx: RevIdx = FromStr::from_str("555").expect("Valid string");
        assert_eq!(idx, RevIdx(555));
    }

    #[test]
    fn fromstr_bad1() {
        match RevIdx::from_str("abc123") {
            Ok(x) => panic!("unexpected success with {:?}", x),
            Err(err) => println!("ok {:?}", err),
        }
    }

    #[test]
    fn fromstr_bad2() {
        match RevIdx::from_str("-1") {
            Ok(x) => panic!("unexpected success with {:?}", x),
            Err(err) => println!("ok {:?}", err),
        }
    }
}
