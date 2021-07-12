/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::blame::{BlameMaybeRejected, BlameRange as BlameRangeV1, BlameRejected};
use mononoke_types::blame_v2::{BlameRange as BlameRangeV2, BlameRanges as BlameRangesV2, BlameV2};
use mononoke_types::{ChangesetId, MPath};

pub enum CompatBlame {
    V1(BlameMaybeRejected),
    V2(BlameV2),
}

impl CompatBlame {
    pub fn ranges(&self) -> Result<CompatBlameRanges<'_>, BlameRejected> {
        match self {
            CompatBlame::V1(BlameMaybeRejected::Rejected(rejected)) => Err(*rejected),
            CompatBlame::V1(BlameMaybeRejected::Blame(blame)) => {
                Ok(CompatBlameRanges::V1(blame.ranges().iter()))
            }
            CompatBlame::V2(blame) => Ok(CompatBlameRanges::V2(blame.ranges()?)),
        }
    }
}

pub enum CompatBlameRanges<'a> {
    V1(std::slice::Iter<'a, BlameRangeV1>),
    V2(BlameRangesV2<'a>),
}

pub struct CompatBlameRange<'a> {
    pub offset: u32,
    pub length: u32,
    pub csid: ChangesetId,
    pub path: &'a MPath,
    pub origin_offset: u32,
}

impl<'a> From<&'a BlameRangeV1> for CompatBlameRange<'a> {
    fn from(range: &'a BlameRangeV1) -> Self {
        CompatBlameRange {
            offset: range.offset,
            length: range.length,
            csid: range.csid,
            path: &range.path,
            origin_offset: range.origin_offset,
        }
    }
}

impl<'a> From<BlameRangeV2<'a>> for CompatBlameRange<'a> {
    fn from(range: BlameRangeV2<'a>) -> Self {
        CompatBlameRange {
            offset: range.offset,
            length: range.length,
            csid: range.csid,
            path: range.path,
            origin_offset: range.origin_offset,
        }
    }
}

impl<'a> Iterator for CompatBlameRanges<'a> {
    type Item = CompatBlameRange<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            CompatBlameRanges::V1(iter) => iter.next().map(CompatBlameRange::from),
            CompatBlameRanges::V2(ranges) => ranges.next().map(CompatBlameRange::from),
        }
    }
}
