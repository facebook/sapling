/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;

use mononoke_types::blame::BlameLines as BlameLinesV1;
use mononoke_types::blame::BlameMaybeRejected;
use mononoke_types::blame::BlameRange as BlameRangeV1;
use mononoke_types::blame::BlameRejected;
use mononoke_types::blame_v2::BlameLine as BlameLineV2;
use mononoke_types::blame_v2::BlameLineParent;
use mononoke_types::blame_v2::BlameLines as BlameLinesV2;
use mononoke_types::blame_v2::BlameRange as BlameRangeV2;
use mononoke_types::blame_v2::BlameRanges as BlameRangesV2;
use mononoke_types::blame_v2::BlameV2;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;

#[derive(Clone)]
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

    pub fn lines(&self) -> Result<CompatBlameLines<'_>, BlameRejected> {
        match self {
            CompatBlame::V1(BlameMaybeRejected::Rejected(rejected)) => Err(*rejected),
            CompatBlame::V1(BlameMaybeRejected::Blame(blame)) => {
                Ok(CompatBlameLines::V1(blame.lines()))
            }
            CompatBlame::V2(blame) => Ok(CompatBlameLines::V2(blame.lines()?)),
        }
    }

    pub fn changeset_ids(&self) -> Result<Vec<(ChangesetId, u32)>, BlameRejected> {
        match self {
            CompatBlame::V1(BlameMaybeRejected::Rejected(rejected)) => Err(*rejected),
            CompatBlame::V1(BlameMaybeRejected::Blame(blame)) => Ok(blame
                .ranges()
                .iter()
                .map(|range| range.csid)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .zip(0..)
                .collect()),
            CompatBlame::V2(blame) => Ok(blame.changeset_ids()?.collect()),
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

pub enum CompatBlameLines<'a> {
    V1(BlameLinesV1<'a>),
    V2(BlameLinesV2<'a>),
}

pub struct CompatBlameLine<'a> {
    pub changeset_id: ChangesetId,
    pub path: &'a MPath,
    pub origin_offset: u32,
    pub changeset_index: Option<u32>,
    pub parent: Option<BlameLineParent<'a>>,
}

impl<'a> From<(ChangesetId, &'a MPath, u32)> for CompatBlameLine<'a> {
    fn from((changeset_id, path, origin_offset): (ChangesetId, &'a MPath, u32)) -> Self {
        CompatBlameLine {
            changeset_id,
            path,
            origin_offset,
            changeset_index: None,
            parent: None,
        }
    }
}

impl<'a> From<BlameLineV2<'a>> for CompatBlameLine<'a> {
    fn from(blame_line: BlameLineV2<'a>) -> Self {
        CompatBlameLine {
            changeset_id: *blame_line.changeset_id,
            path: blame_line.path,
            origin_offset: blame_line.origin_offset,
            changeset_index: Some(blame_line.changeset_index),
            parent: blame_line.parent,
        }
    }
}

impl<'a> Iterator for CompatBlameLines<'a> {
    type Item = CompatBlameLine<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            CompatBlameLines::V1(lines) => lines.next().map(CompatBlameLine::from),
            CompatBlameLines::V2(lines) => lines.next().map(CompatBlameLine::from),
        }
    }
}
