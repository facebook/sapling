/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use anyhow::{format_err, Error};
use ascii::{AsciiChar, AsciiString};
use quickcheck::{Arbitrary, Gen};
use rand::Rng;
use sql::mysql_async::{
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};
use std::convert::TryFrom;
use std::fmt;
use std::ops::{Bound, Range, RangeBounds, RangeFrom, RangeFull};

/// This enum represents how fresh you want results to be. MostRecent will go to the master, so you
/// normally don't want to issue queries using MostRecent unless you have a very good reason.
/// MaybeStale will go to a replica, which might lag behind the master (there is no SLA on
/// replication lag). MaybeStale reads might also be served from a local cache.

#[derive(Debug, Eq, PartialEq, Clone, Copy)]
pub enum Freshness {
    MostRecent,
    MaybeStale,
}

impl Arbitrary for Freshness {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        use Freshness::*;

        match g.gen_range(0, 2) {
            0 => MostRecent,
            1 => MaybeStale,
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct Bookmark {
    pub name: BookmarkName,
    pub hg_kind: BookmarkHgKind,
}

impl Arbitrary for Bookmark {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        let name = BookmarkName::arbitrary(g);
        Self {
            name,
            hg_kind: Arbitrary::arbitrary(g),
        }
    }
}

impl Bookmark {
    pub fn new(name: BookmarkName, hg_kind: BookmarkHgKind) -> Self {
        Bookmark { name, hg_kind }
    }

    pub fn into_name(self) -> BookmarkName {
        self.name
    }

    pub fn name(&self) -> &BookmarkName {
        &self.name
    }

    pub fn hg_kind(&self) -> &BookmarkHgKind {
        &self.hg_kind
    }

    pub fn publishing(&self) -> bool {
        use BookmarkHgKind::*;

        match self.hg_kind {
            Scratch => false,
            PublishingNotPullDefault => true,
            PullDefault => true,
        }
    }

    pub fn pull_default(&self) -> bool {
        use BookmarkHgKind::*;

        match self.hg_kind {
            Scratch => false,
            PublishingNotPullDefault => false,
            PullDefault => true,
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq, Ord, PartialOrd)]
pub struct BookmarkName {
    bookmark: AsciiString,
}

impl fmt::Display for BookmarkName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.bookmark)
    }
}

impl Arbitrary for BookmarkName {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        // NOTE: We use a specific large size here because our tests exercise DB Bookmarks, which
        // require unique names in the DB.
        let size = 128;
        let mut bookmark = AsciiString::with_capacity(size);
        for _ in 0..size {
            bookmark.push(ascii_ext::AsciiChar::arbitrary(g).0);
        }
        Self { bookmark }
    }
}

impl BookmarkName {
    pub fn new<B: AsRef<str>>(bookmark: B) -> Result<Self, Error> {
        Ok(Self {
            bookmark: AsciiString::from_ascii(bookmark.as_ref())
                .map_err(|bytes| format_err!("non-ascii bookmark name: {:?}", bytes))?,
        })
    }

    pub fn new_ascii(bookmark: AsciiString) -> Self {
        Self { bookmark }
    }

    pub fn as_ascii(&self) -> &AsciiString {
        &self.bookmark
    }

    pub fn to_string(&self) -> String {
        self.bookmark.clone().into()
    }

    pub fn into_string(self) -> String {
        self.bookmark.into()
    }

    pub fn into_byte_vec(self) -> Vec<u8> {
        self.bookmark.into()
    }

    pub fn as_str(&self) -> &str {
        self.bookmark.as_str()
    }
}

impl TryFrom<&str> for BookmarkName {
    type Error = Error;

    fn try_from(s: &str) -> Result<Self, Error> {
        Self::new(s)
    }
}

impl From<BookmarkName> for Value {
    fn from(bookmark: BookmarkName) -> Self {
        Value::Bytes(bookmark.bookmark.into())
    }
}

impl ConvIr<BookmarkName> for BookmarkName {
    fn new(v: Value) -> Result<Self, FromValueError> {
        match v {
            Value::Bytes(bytes) => AsciiString::from_ascii(bytes)
                .map_err(|err| FromValueError(Value::Bytes(err.into_source())))
                .map(BookmarkName::new_ascii),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> BookmarkName {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for BookmarkName {
    type Intermediate = BookmarkName;
}

impl From<BookmarkPrefix> for Value {
    fn from(bookmark_prefix: BookmarkPrefix) -> Self {
        Value::Bytes(bookmark_prefix.bookmark_prefix.into())
    }
}

/// Describes the behavior of a Bookmark in Mercurial operations.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Copy)]
pub enum BookmarkHgKind {
    Scratch,
    PublishingNotPullDefault,
    /// NOTE: PullDefault implies Publishing.
    PullDefault,
}

impl std::fmt::Display for BookmarkHgKind {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        use BookmarkHgKind::*;

        let s = match self {
            Scratch => "scratch",
            PublishingNotPullDefault => "publishing",
            PullDefault => "pull_default",
        };

        write!(f, "{}", s)
    }
}

const SCRATCH_HG_KIND: &[u8] = b"scratch";
const PUBLISHING_HG_KIND: &[u8] = b"publishing";
const PULL_DEFAULT_HG_KIND: &[u8] = b"pull_default";

impl ConvIr<BookmarkHgKind> for BookmarkHgKind {
    fn new(v: Value) -> Result<Self, FromValueError> {
        use BookmarkHgKind::*;

        match v {
            Value::Bytes(ref b) if b == &SCRATCH_HG_KIND => Ok(Scratch),
            Value::Bytes(ref b) if b == &PUBLISHING_HG_KIND => Ok(PublishingNotPullDefault),
            Value::Bytes(ref b) if b == &PULL_DEFAULT_HG_KIND => Ok(PullDefault),
            v => Err(FromValueError(v)),
        }
    }

    fn commit(self) -> BookmarkHgKind {
        self
    }

    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for BookmarkHgKind {
    type Intermediate = BookmarkHgKind;
}

impl From<BookmarkHgKind> for Value {
    fn from(bookmark_update_reason: BookmarkHgKind) -> Self {
        use BookmarkHgKind::*;

        match bookmark_update_reason {
            Scratch => Value::Bytes(SCRATCH_HG_KIND.to_vec()),
            PublishingNotPullDefault => Value::Bytes(PUBLISHING_HG_KIND.to_vec()),
            PullDefault => Value::Bytes(PULL_DEFAULT_HG_KIND.to_vec()),
        }
    }
}

impl Arbitrary for BookmarkHgKind {
    fn arbitrary<G: Gen>(g: &mut G) -> Self {
        use BookmarkHgKind::*;

        match g.gen_range(0, 3) {
            0 => Scratch,
            1 => PublishingNotPullDefault,
            2 => PullDefault,
            _ => unreachable!(),
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BookmarkPrefix {
    bookmark_prefix: AsciiString,
}

impl fmt::Display for BookmarkPrefix {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.bookmark_prefix)
    }
}

pub enum BookmarkPrefixRange {
    Range(Range<BookmarkName>),
    RangeFrom(RangeFrom<BookmarkName>),
    RangeFull(RangeFull),
}

impl RangeBounds<BookmarkName> for BookmarkPrefixRange {
    fn start_bound(&self) -> Bound<&BookmarkName> {
        use BookmarkPrefixRange::*;
        match self {
            Range(r) => r.start_bound(),
            RangeFrom(r) => r.start_bound(),
            RangeFull(r) => r.start_bound(),
        }
    }

    fn end_bound(&self) -> Bound<&BookmarkName> {
        use BookmarkPrefixRange::*;
        match self {
            Range(r) => r.end_bound(),
            RangeFrom(r) => r.end_bound(),
            RangeFull(r) => r.end_bound(),
        }
    }
}

impl BookmarkPrefix {
    pub fn new<B: AsRef<str>>(bookmark_prefix: B) -> Result<Self, Error> {
        Ok(Self {
            bookmark_prefix: AsciiString::from_ascii(bookmark_prefix.as_ref())
                .map_err(|bytes| format_err!("non-ascii bookmark prefix: {:?}", bytes))?,
        })
    }

    pub fn new_ascii(bookmark_prefix: AsciiString) -> Self {
        Self { bookmark_prefix }
    }

    pub fn empty() -> Self {
        Self {
            bookmark_prefix: AsciiString::default(),
        }
    }

    pub fn to_string(&self) -> String {
        self.bookmark_prefix.clone().into()
    }

    pub fn is_empty(&self) -> bool {
        self.bookmark_prefix.is_empty()
    }

    pub fn to_range(&self) -> BookmarkPrefixRange {
        match prefix_to_range_end(self.bookmark_prefix.clone()) {
            Some(range_end) => {
                let range = Range {
                    start: BookmarkName::new_ascii(self.bookmark_prefix.clone()),
                    end: BookmarkName::new_ascii(range_end),
                };
                BookmarkPrefixRange::Range(range)
            }
            None => match self.bookmark_prefix.len() {
                0 => BookmarkPrefixRange::RangeFull(RangeFull),
                _ => {
                    let range = RangeFrom {
                        start: BookmarkName::new_ascii(self.bookmark_prefix.clone()),
                    };
                    BookmarkPrefixRange::RangeFrom(range)
                }
            },
        }
    }
}

fn prefix_to_range_end(mut prefix: AsciiString) -> Option<AsciiString> {
    // If we have a prefix, then we need to take the last character of the prefix, increment it by
    // 1, then take that as an ASCII char. So, if you prefix is foobarA, then the range will be
    // from foobarA (inclusive) to foobarB (exclusive). Basically, what we're trying to implement
    // here is a little bit like Ruby's str#next.
    loop {
        match prefix.pop() {
            Some(chr) => match AsciiChar::from_ascii(chr.as_byte() + 1) {
                Ok(next_chr) => {
                    // Happy path, we found the next character, so just put that in and move on.
                    prefix.push(next_chr);
                    return Some(prefix);
                }
                Err(_) => {
                    // The last character doesn't fit in ASCII (i.e. it's DEL). This means we have
                    // something like foobaA[DEL]. In this case, we need to set the bound to be the
                    // character after the one before the DEL, so we want foobaB[DEL]
                    continue;
                }
            },
            None => {
                // We exhausted the entire string. This will only happen if the string is 0 or more
                // DEL characters. In this case, return the fact that no next string can be
                // produced.
                return None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;

    quickcheck! {
        fn test_prefix_range_contains_self(bookmark: Bookmark) -> bool {
            let prefix = BookmarkPrefix::new_ascii(bookmark.name().as_ascii().clone());
            prefix.to_range().contains(bookmark.name())
        }

        fn test_prefix_range_contains_its_suffixes(bookmark: Bookmark, more: ascii_ext::AsciiString) -> bool {
            let prefix = BookmarkPrefix::new_ascii(bookmark.name().as_ascii().clone());
            let mut name = bookmark.name().as_ascii().clone();
            name.push_str(&more.0);
            prefix.to_range().contains(&BookmarkName::new_ascii(name))
        }

        fn test_prefix_range_does_not_contains_its_prefixes(bookmark: Bookmark, chr: ascii_ext::AsciiChar) -> bool {
            let mut prefix = bookmark.name().as_ascii().clone();
            prefix.push(chr.0);
            let prefix = BookmarkPrefix::new_ascii(prefix);

            !prefix.to_range().contains(bookmark.name())
        }

        fn test_empty_range_contains_any(bookmark: Bookmark) -> bool {
            let prefix = BookmarkPrefix::empty();
            prefix.to_range().contains(bookmark.name())
        }
    }
}
