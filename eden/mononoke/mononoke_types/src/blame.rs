/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::thrift;
use crate::typed_hash::BlobstoreKey;
use crate::typed_hash::FileUnodeId;
use crate::typed_hash::MononokeId;
use crate::ChangesetId;
use crate::MPath;
use anyhow::bail;
use anyhow::format_err;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::Loadable;
use blobstore::LoadableError;
use context::CoreContext;
use fbthrift::compact_protocol;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::str::FromStr;
use thiserror::Error;
use xdiff::diff_hunks;
use xdiff::Hunk;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BlameId(FileUnodeId);

impl BlameId {
    pub fn blobstore_key(&self) -> String {
        format!("blame.{}", self.0.blobstore_key())
    }
    pub fn sampling_fingerprint(&self) -> u64 {
        self.0.sampling_fingerprint()
    }
}

impl FromStr for BlameId {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(BlameId(FileUnodeId::from_str(s)?))
    }
}

impl From<FileUnodeId> for BlameId {
    fn from(file_unode_id: FileUnodeId) -> Self {
        BlameId(file_unode_id)
    }
}

impl From<BlameId> for FileUnodeId {
    fn from(blame_id: BlameId) -> Self {
        blame_id.0
    }
}

impl AsRef<FileUnodeId> for BlameId {
    fn as_ref(&self) -> &FileUnodeId {
        &self.0
    }
}

#[async_trait]
impl Loadable for BlameId {
    type Value = BlameMaybeRejected;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        let blobstore_key = self.blobstore_key();
        let fetch = blobstore.get(ctx, &blobstore_key);

        let bytes = fetch.await?.ok_or(LoadableError::Missing(blobstore_key))?;
        let blame_t = compact_protocol::deserialize(bytes.as_raw_bytes().as_ref())?;
        let blame = BlameMaybeRejected::from_thrift(blame_t)?;
        Ok(blame)
    }
}

/// Store blame object as associated blame to provided FileUnodeId
///
/// NOTE: `Blame` is not a `Storable` object and can only be assoicated with
///       some file unode id.
pub async fn store_blame<'a, B: Blobstore>(
    ctx: &'a CoreContext,
    blobstore: &'a B,
    file_unode_id: FileUnodeId,
    blame: BlameMaybeRejected,
) -> Result<BlameId> {
    let blame_t = blame.into_thrift();
    let data = compact_protocol::serialize(&blame_t);
    let data = BlobstoreBytes::from_bytes(data);
    let blame_id = BlameId::from(file_unode_id);
    blobstore.put(ctx, blame_id.blobstore_key(), data).await?;
    Ok(blame_id)
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash, Error)]
pub enum BlameRejected {
    #[error("Blame was not generated because file was too big")]
    TooBig,
    #[error("Blame was not generated because file was marked as binary")]
    Binary,
}

impl BlameRejected {
    pub fn into_thrift(self) -> thrift::BlameRejected {
        match self {
            BlameRejected::TooBig => thrift::BlameRejected::TooBig,
            BlameRejected::Binary => thrift::BlameRejected::Binary,
        }
    }

    pub fn from_thrift(rejected_t: thrift::BlameRejected) -> Result<Self, Error> {
        let rejected = match rejected_t {
            thrift::BlameRejected::TooBig => BlameRejected::TooBig,
            thrift::BlameRejected::Binary => BlameRejected::Binary,
            thrift::BlameRejected(id) => {
                return Err(format_err!(
                    "BlameRejected contains unknown variant: {}",
                    id
                ));
            }
        };
        Ok(rejected)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum BlameMaybeRejected {
    Rejected(BlameRejected),
    Blame(Blame),
}

impl BlameMaybeRejected {
    pub fn into_blame(self) -> Result<Blame, Error> {
        match self {
            BlameMaybeRejected::Blame(blame) => Ok(blame),
            BlameMaybeRejected::Rejected(reason) => Err(reason.into()),
        }
    }

    pub fn from_thrift(blame_maybe_rejected_t: thrift::BlameMaybeRejected) -> Result<Self, Error> {
        match blame_maybe_rejected_t {
            thrift::BlameMaybeRejected::Rejected(rejected_t) => Ok(BlameMaybeRejected::Rejected(
                BlameRejected::from_thrift(rejected_t)?,
            )),
            thrift::BlameMaybeRejected::Blame(blame_t) => {
                Ok(BlameMaybeRejected::Blame(Blame::from_thrift(blame_t)?))
            }
            thrift::BlameMaybeRejected::UnknownField(id) => Err(format_err!(
                "BlameMaybeRejected contains unknown variant with id: {}",
                id
            )),
        }
    }

    pub fn into_thrift(self) -> thrift::BlameMaybeRejected {
        match self {
            BlameMaybeRejected::Blame(blame) => {
                thrift::BlameMaybeRejected::Blame(blame.into_thrift())
            }
            BlameMaybeRejected::Rejected(rejected) => {
                thrift::BlameMaybeRejected::Rejected(rejected.into_thrift())
            }
        }
    }
}

impl From<Blame> for BlameMaybeRejected {
    fn from(blame: Blame) -> BlameMaybeRejected {
        BlameMaybeRejected::Blame(blame)
    }
}

impl From<BlameRejected> for BlameMaybeRejected {
    fn from(rejected: BlameRejected) -> BlameMaybeRejected {
        BlameMaybeRejected::Rejected(rejected)
    }
}

impl TryFrom<BlameMaybeRejected> for Blame {
    type Error = BlameRejected;

    fn try_from(blame: BlameMaybeRejected) -> Result<Blame, BlameRejected> {
        match blame {
            BlameMaybeRejected::Blame(blame) => Ok(blame),
            BlameMaybeRejected::Rejected(rejected) => Err(rejected),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BlameRange {
    pub offset: u32,
    pub length: u32,
    pub csid: ChangesetId,
    pub path: MPath,
    pub origin_offset: u32,
}

impl BlameRange {
    fn split_at(self, offset: u32) -> (Option<BlameRange>, Option<BlameRange>) {
        if offset <= self.offset {
            (None, Some(self))
        } else if offset >= self.offset + self.length {
            (Some(self), None)
        } else {
            let left = BlameRange {
                offset: self.offset,
                length: offset - self.offset,
                csid: self.csid,
                path: self.path.clone(),
                origin_offset: self.origin_offset,
            };
            let right = BlameRange {
                offset,
                length: self.length - left.length,
                csid: self.csid,
                path: self.path,
                origin_offset: self.origin_offset + left.length,
            };
            (Some(left), Some(right))
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Blame {
    ranges: Vec<BlameRange>,
}

impl Blame {
    fn new(ranges: Vec<BlameRange>) -> Result<Self, Error> {
        let mut offset = 0u32;
        for range in ranges.iter() {
            if range.offset != offset {
                bail!("ranges could not form valid Blame object");
            }
            offset += range.length;
        }
        Ok(Blame { ranges })
    }

    pub fn from_thrift(blame_t: thrift::Blame) -> Result<Blame, Error> {
        let thrift::Blame {
            ranges: ranges_t,
            paths: paths_t,
        } = blame_t;
        let paths = paths_t
            .into_iter()
            .map(MPath::from_thrift)
            .collect::<Result<Vec<_>, _>>()?;
        let (_length, ranges) =
            ranges_t
                .into_iter()
                .fold(Ok::<_, Error>((0, Vec::new())), |acc, range_t| {
                    let (mut offset, mut ranges) = acc?;
                    let thrift::BlameRange {
                        length,
                        csid: csid_t,
                        path: path_index,
                        origin_offset,
                    } = range_t;
                    let csid = ChangesetId::from_thrift(csid_t)?;
                    let path = paths
                        .get(path_index.0 as usize)
                        .ok_or_else(|| Error::msg("invalid blame path index"))?
                        .clone();
                    ranges.push(BlameRange {
                        offset,
                        length: length as u32,
                        csid,
                        path,
                        origin_offset: origin_offset as u32,
                    });
                    offset += length as u32;
                    Ok((offset, ranges))
                })?;
        Blame::new(ranges)
    }

    pub fn into_thrift(self) -> thrift::Blame {
        let mut paths_indices = HashMap::new();
        let mut paths = Vec::new();
        let ranges = self
            .ranges
            .into_iter()
            .map(|range| {
                let BlameRange {
                    length,
                    csid,
                    path,
                    origin_offset,
                    ..
                } = range;
                let index = match paths_indices.get(&path) {
                    Some(&index) => index,
                    None => {
                        let index = paths.len() as i32;
                        paths_indices.insert(path.clone(), index);
                        paths.push(path.into_thrift());
                        index
                    }
                };
                thrift::BlameRange {
                    length: length as i32,
                    csid: csid.into_thrift(),
                    path: thrift::BlamePath(index),
                    origin_offset: origin_offset as i32,
                }
            })
            .collect();
        thrift::Blame { ranges, paths }
    }

    pub fn ranges(&self) -> &Vec<BlameRange> {
        &self.ranges
    }

    pub fn from_parents<C>(
        csid: ChangesetId,
        content: C,
        path: MPath,
        parents: Vec<(C, Blame)>,
    ) -> Result<Blame, Error>
    where
        C: AsRef<[u8]>,
    {
        if parents.is_empty() {
            return Blame::from_no_parents(csid, content, path);
        }
        let mut blames = parents
            .into_iter()
            .map(|(parent_content, parent_blame)| {
                Blame::from_single_parent(
                    csid,
                    content.as_ref(),
                    path.clone(),
                    parent_content.as_ref(),
                    parent_blame,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;

        if blames.len() == 1 {
            if let Some(blame) = blames.pop() {
                return Ok(blame);
            }
            unreachable!();
        }

        blame_merge(csid, blames)
    }

    fn from_no_parents<C: AsRef<[u8]>>(
        csid: ChangesetId,
        content: C,
        path: MPath,
    ) -> Result<Blame, Error> {
        // calculating length by diffing with empty content, so number of lines
        // would be calculcated the same way as xdiff doest it.
        let length = match diff_hunks(&b""[..], content.as_ref()).first() {
            None => 0,
            Some(hunk) => (hunk.add.end - hunk.add.start) as u32,
        };
        Blame::new(vec![BlameRange {
            offset: 0,
            length,
            csid,
            path,
            origin_offset: 0,
        }])
    }

    fn from_single_parent<C: AsRef<[u8]>>(
        csid: ChangesetId,
        content: C,
        path: MPath,
        parent_content: C,
        parent_blame: Blame,
    ) -> Result<Blame, Error> {
        // Hunks comming from `diff_hunks` have two associated ranges `add` and `remove`
        // they always talk about the same place in a code (you are basically replace
        // `add` with `remove`. That is why it is safe to just add new range from `add`
        // field after removing range from `remove`. Also note that ranges after transformation
        // below **do not** contain vaild offset.

        let ranges = VecDeque::from(parent_blame.ranges);
        let (mut ranges, rest, _) = diff_hunks(parent_content, content).into_iter().fold(
            (Vec::new(), ranges, 0u32),
            |(mut output, ranges, mut origin_offset), Hunk { add, remove }| {
                // add unaffected ranges
                let (unaffected, mid) = blame_ranges_split_at(ranges, remove.start as u32);
                for range in unaffected {
                    origin_offset += range.length;
                    output.push(range);
                }

                // skip removed ranges
                let right = if remove.end > remove.start {
                    let (_removed_blame, right) = blame_ranges_split_at(mid, remove.end as u32);
                    right
                } else {
                    mid
                };

                // add new range
                if add.end > add.start {
                    let length = (add.end - add.start) as u32;
                    output.push(BlameRange {
                        // we do not care about offset here since all the ranges at this point
                        // might include incorrect offset. It is fixed lower in the code of this
                        // function.
                        offset: 0,
                        length,
                        csid,
                        path: path.clone(),
                        origin_offset,
                    });
                    origin_offset += length;
                }

                (output, right, origin_offset)
            },
        );
        ranges.extend(rest);

        // merge adjacent ranges with identical changeset id and recalculate offsets
        let (_length, ranges): (u32, Vec<BlameRange>) =
            ranges
                .into_iter()
                .fold((0, Vec::new()), |(mut offset, mut output), range| {
                    match output.last_mut() {
                        Some(ref mut last)
                            if last.csid == range.csid
                                && last.origin_offset + last.length == range.origin_offset =>
                        {
                            last.length += range.length;
                        }
                        _ => {
                            output.push(BlameRange { offset, ..range });
                        }
                    }
                    offset += range.length;
                    (offset, output)
                });

        Blame::new(ranges)
    }

    pub fn lines<'a>(&'a self) -> BlameLines<'a> {
        BlameLines::new(&self.ranges)
    }

    pub fn annotate(&self, content: &str) -> Result<String, Error> {
        if content.is_empty() {
            return Ok(String::new());
        }

        let mut result = String::new();
        let mut ranges = self.ranges.iter();
        let mut range = ranges
            .next()
            .ok_or_else(|| Error::msg("empty blame for non empty content"))?;
        for (index, line) in content.lines().enumerate() {
            if index as u32 >= range.offset + range.length {
                range = ranges
                    .next()
                    .ok_or_else(|| Error::msg("not enough ranges in a blame"))?;
            }
            result.push_str(&range.csid.to_string()[..12]);
            result.push_str(": ");
            result.push_str(line);
            result.push('\n');
        }

        Ok(result)
    }
}

/// Split blame ranges at a specified offset
fn blame_ranges_split_at(
    mut ranges: VecDeque<BlameRange>,
    offset: u32,
) -> (VecDeque<BlameRange>, VecDeque<BlameRange>) {
    let mut left = VecDeque::new();

    while let Some(range) = ranges.pop_front() {
        if range.offset + range.length < offset {
            left.push_back(range);
        } else {
            let (left_range, right_range) = range.split_at(offset);
            left.extend(left_range);
            if let Some(right_range) = right_range {
                ranges.push_front(right_range);
            }
            break;
        }
    }

    (left, ranges)
}

/// Merge multiple blames into a single.
///
/// All blames are assumed to be generated by running `blame_single_parent`
/// for the conntent associated with provided `csid` (That is they have
/// the same total size).
///
/// This code converts each provided blame to iterator of lines. And then
/// this lines are merged one by one. Provided `csid` is the ChangesetId
/// of the new blame being constructed. Logic of merging lines goes as
/// follows:
///   - All lines contain `csid` - merged line will contain `csid`
///   - Otherwise first changeset id not equeal to `csid` will be taken
///     together with its associated path.
/// Last step is just convertion of iterator of lines back to `Blame` object.
///
/// NOTE: We are only cloning paths once per generated `BlameRange` object.
///       And all intermediate iterators are working with `&MPath` here.
fn blame_merge(csid: ChangesetId, blames: Vec<Blame>) -> Result<Blame, Error> {
    let iters: Vec<_> = blames.iter().map(|blame| blame.lines()).collect();
    let (_length, ranges) = BlameMergeLines::new(csid, iters).fold(
        (0, Vec::new()),
        |(mut offset, mut output), (csid, path, origin_offset)| -> (u32, Vec<BlameRange>) {
            match output.last_mut() {
                Some(ref mut last) if last.csid == csid && &last.path == path => {
                    last.length += 1;
                }
                _ => {
                    output.push(BlameRange {
                        offset,
                        length: 1,
                        csid,
                        path: path.clone(),
                        origin_offset,
                    });
                }
            }
            offset += 1;
            (offset, output)
        },
    );
    Blame::new(ranges)
}

/// Iterator over balme object as if it was just a list of lines with associated
/// changeset id and path. Implementation is not cloning anyting.
pub struct BlameLines<'a> {
    ranges: &'a Vec<BlameRange>,
    ranges_index: usize,
    index: u32,
}

impl<'a> BlameLines<'a> {
    fn new(ranges: &'a Vec<BlameRange>) -> BlameLines<'a> {
        BlameLines {
            ranges,
            ranges_index: 0,
            index: 0,
        }
    }
}

impl<'a> Iterator for BlameLines<'a> {
    type Item = (ChangesetId, &'a MPath, u32);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.ranges.get(self.ranges_index) {
                None => return None,
                Some(range) if self.index < range.length => {
                    self.index += 1;
                    return Some((
                        range.csid,
                        &range.path,
                        range.origin_offset + self.index - 1,
                    ));
                }
                _ => {
                    self.ranges_index += 1;
                    self.index = 0;
                }
            }
        }
    }
}

/// Merge iterator on a list of `BlameLines` iterators. For more details
/// see description of `blame_merge` function.
struct BlameMergeLines<'a> {
    csid: ChangesetId,
    lines: Vec<BlameLines<'a>>,
}

impl<'a> BlameMergeLines<'a> {
    fn new(csid: ChangesetId, lines: Vec<BlameLines<'a>>) -> Self {
        Self { csid, lines }
    }
}

impl<'a> Iterator for BlameMergeLines<'a> {
    type Item = (ChangesetId, &'a MPath, u32);

    fn next(&mut self) -> Option<Self::Item> {
        let (last, rest) = self.lines.split_last_mut()?;
        let mut rest = rest.iter_mut();
        while let Some(lines) = rest.next() {
            let (csid, path, origin_offset) = lines.next()?;
            if csid != self.csid {
                // we need to pull all remaining iterators associated with
                // lines with the same index.
                rest.for_each(|lines| {
                    lines.next();
                });
                last.next();
                return Some((csid, path, origin_offset));
            }
        }
        last.next()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::hash::Blake2;

    const ONES_CSID: ChangesetId = ChangesetId::new(Blake2::from_byte_array([0x11; 32]));
    const TWOS_CSID: ChangesetId = ChangesetId::new(Blake2::from_byte_array([0x22; 32]));
    const THREES_CSID: ChangesetId = ChangesetId::new(Blake2::from_byte_array([0x33; 32]));
    const FOURS_CSID: ChangesetId = ChangesetId::new(Blake2::from_byte_array([0x44; 32]));

    #[test]
    fn test_blame_lines() -> Result<(), Error> {
        let p0 = MPath::new("path/zero")?;
        let p1 = MPath::new("path/one")?;

        let blame = Blame::new(vec![
            BlameRange {
                offset: 0,
                length: 2,
                csid: ONES_CSID,
                path: p0.clone(),
                origin_offset: 3,
            },
            BlameRange {
                offset: 2,
                length: 3,
                csid: TWOS_CSID,
                path: p1.clone(),
                origin_offset: 2,
            },
        ])?;

        let lines: Vec<_> = blame
            .lines()
            .map(|(csid, path, origin_offset)| (csid, path.clone(), origin_offset))
            .collect();
        let reference = vec![
            (ONES_CSID, p0.clone(), 3),
            (ONES_CSID, p0, 4),
            (TWOS_CSID, p1.clone(), 2),
            (TWOS_CSID, p1.clone(), 3),
            (TWOS_CSID, p1, 4),
        ];
        assert_eq!(lines, reference);

        Ok(())
    }

    #[test]
    fn test_blame_merge_lines() -> Result<(), Error> {
        // Merging blame generated for to parents of changeset 1.
        // Assuming changset graph:
        //    1
        //   / \
        //  2   3
        //   \ /
        //    4
        // Blame merge result:
        //    b0     b1         result
        //  |  2  ||  4  |      |  2  |
        //  |  4  ||  4  |      |  4  |
        //  |  1  ||  1  |  ->  |  1  |
        //  |  1  ||  3  |      |  3  |
        //  |  1  ||  3  |      |  3  |
        //  |  4  ||  3  |      |  4  |

        let p0 = MPath::new("path/zero")?;
        let p1 = MPath::new("path/one")?;

        let b0 = Blame::new(vec![
            BlameRange {
                offset: 0,
                length: 1,
                csid: TWOS_CSID,
                path: p0.clone(),
                origin_offset: 5,
            },
            BlameRange {
                offset: 1,
                length: 1,
                csid: FOURS_CSID,
                path: p0.clone(),
                origin_offset: 1,
            },
            BlameRange {
                offset: 2,
                length: 3,
                csid: ONES_CSID,
                path: p0.clone(),
                origin_offset: 31,
            },
            BlameRange {
                offset: 5,
                length: 1,
                csid: FOURS_CSID,
                path: p0.clone(),
                origin_offset: 5,
            },
        ])?;

        let b1 = Blame::new(vec![
            BlameRange {
                offset: 0,
                length: 2,
                csid: FOURS_CSID,
                path: p0.clone(),
                origin_offset: 0,
            },
            BlameRange {
                offset: 2,
                length: 1,
                csid: ONES_CSID,
                path: p0.clone(),
                origin_offset: 31,
            },
            BlameRange {
                offset: 3,
                length: 3,
                csid: THREES_CSID,
                path: p1.clone(),
                origin_offset: 3,
            },
        ])?;

        let reference = Blame::new(vec![
            BlameRange {
                offset: 0,
                length: 1,
                csid: TWOS_CSID,
                path: p0.clone(),
                origin_offset: 5,
            },
            BlameRange {
                offset: 1,
                length: 1,
                csid: FOURS_CSID,
                path: p0.clone(),
                origin_offset: 1,
            },
            BlameRange {
                offset: 2,
                length: 1,
                csid: ONES_CSID,
                path: p0.clone(),
                origin_offset: 31,
            },
            BlameRange {
                offset: 3,
                length: 2,
                csid: THREES_CSID,
                path: p1,
                origin_offset: 3,
            },
            BlameRange {
                offset: 5,
                length: 1,
                csid: FOURS_CSID,
                path: p0,
                origin_offset: 5,
            },
        ])?;

        assert_eq!(blame_merge(ONES_CSID, vec![b0, b1])?, reference);

        Ok(())
    }

    #[test]
    fn test_thrift() -> Result<(), Error> {
        let p0 = MPath::new("path/zero")?;
        let p1 = MPath::new("path/oen")?;

        let blame = Blame {
            ranges: vec![
                BlameRange {
                    offset: 0,
                    length: 1,
                    csid: TWOS_CSID,
                    path: p0.clone(),
                    origin_offset: 5,
                },
                BlameRange {
                    offset: 1,
                    length: 1,
                    csid: FOURS_CSID,
                    path: p0.clone(),
                    origin_offset: 31,
                },
                BlameRange {
                    offset: 2,
                    length: 1,
                    csid: ONES_CSID,
                    path: p0.clone(),
                    origin_offset: 127,
                },
                BlameRange {
                    offset: 3,
                    length: 2,
                    csid: THREES_CSID,
                    path: p1,
                    origin_offset: 15,
                },
                BlameRange {
                    offset: 5,
                    length: 1,
                    csid: FOURS_CSID,
                    path: p0,
                    origin_offset: 3,
                },
            ],
        };

        let blame_t = blame.clone().into_thrift();
        assert_eq!(Blame::from_thrift(blame_t)?, blame);

        Ok(())
    }

    #[test]
    fn test_blame_add_remove_add_whole_file() -> Result<(), Error> {
        let path = MPath::new("test/file")?;

        let c1 = "one\ntwo\nthree\n";
        let c2 = "";
        let c3 = "one\nthree\nfour";

        let b1 = Blame::from_parents(ONES_CSID, c1, path.clone(), Vec::new())?;
        let b2 = Blame::from_parents(TWOS_CSID, c2, path.clone(), vec![(c1, b1)])?;
        let b3 = Blame::from_parents(THREES_CSID, c3, path.clone(), vec![(c2, b2)])?;

        let b3_reference = Blame::new(vec![BlameRange {
            offset: 0,
            length: 3,
            csid: THREES_CSID,
            path,
            origin_offset: 0,
        }])?;

        assert_eq!(b3_reference, b3);
        Ok(())
    }

    #[test]
    fn test_blame_single_parent() -> Result<(), Error> {
        let path = MPath::new("path")?;

        let c1 = "one\ntwo\nthree\nfour\n";
        let c2 = "one\nfive\nsix\nfour\n";
        let c3 = "seven\none\nsix\neight\nfour\n";

        let b1 = Blame::from_parents(ONES_CSID, c1, path.clone(), Vec::new())?;
        let b2 = Blame::from_parents(TWOS_CSID, c2, path.clone(), vec![(c1, b1)])?;
        let b3 = Blame::from_parents(THREES_CSID, c3, path.clone(), vec![(c2, b2)])?;

        let b3_reference = Blame::new(vec![
            BlameRange {
                offset: 0,
                length: 1,
                csid: THREES_CSID,
                path: path.clone(),
                origin_offset: 0,
            },
            BlameRange {
                offset: 1,
                length: 1,
                csid: ONES_CSID,
                path: path.clone(),
                origin_offset: 0,
            },
            BlameRange {
                offset: 2,
                length: 1,
                csid: TWOS_CSID,
                path: path.clone(),
                origin_offset: 2,
            },
            BlameRange {
                offset: 3,
                length: 1,
                csid: THREES_CSID,
                path: path.clone(),
                origin_offset: 3,
            },
            BlameRange {
                offset: 4,
                length: 1,
                csid: ONES_CSID,
                path,
                origin_offset: 3,
            },
        ])?;

        assert_eq!(b3_reference, b3);
        Ok(())
    }
}
