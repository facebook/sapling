/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::{
    thrift,
    typed_hash::{FileUnodeId, MononokeId},
    ChangesetId, MPath,
};
use anyhow::{bail, format_err, Error};
use blobstore::{Blobstore, BlobstoreBytes, Loadable, LoadableError};
use context::CoreContext;
use fbthrift::compact_protocol;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use std::{collections::HashMap, convert::TryFrom};
use thiserror::Error;
use xdiff::{diff_hunks, Hunk};

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BlameId(FileUnodeId);

impl BlameId {
    pub fn blobstore_key(&self) -> String {
        format!("blame.{}", self.0.blobstore_key())
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

impl Loadable for BlameId {
    type Value = BlameMaybeRejected;

    fn load<B: Blobstore + Clone>(
        &self,
        ctx: CoreContext,
        blobstore: &B,
    ) -> BoxFuture<Self::Value, LoadableError> {
        let blobstore_key = self.blobstore_key();
        blobstore
            .get(ctx, blobstore_key.clone())
            .from_err()
            .and_then(move |bytes| {
                let bytes = bytes.ok_or(LoadableError::Missing(blobstore_key))?;
                let blame_t = compact_protocol::deserialize(bytes.as_bytes().as_ref())?;
                let blame = BlameMaybeRejected::from_thrift(blame_t)?;
                Ok(blame)
            })
            .boxify()
    }
}

/// Store blame object as associated blame to provided FileUnodeId
///
/// NOTE: `Blame` is not a `Storable` object and can only be assoicated with
///       some file unode id.
pub fn store_blame<B: Blobstore + Clone>(
    ctx: CoreContext,
    blobstore: &B,
    file_unode_id: FileUnodeId,
    blame: BlameMaybeRejected,
) -> impl Future<Item = BlameId, Error = Error> {
    let blame_t = blame.into_thrift();
    let data = BlobstoreBytes::from_bytes(compact_protocol::serialize(&blame_t));
    let blame_id = BlameId::from(file_unode_id);
    blobstore
        .put(ctx, blame_id.blobstore_key(), data)
        .map(move |_| blame_id)
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
                ))
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
            };
            let right = BlameRange {
                offset,
                length: self.offset + self.length - offset,
                csid: self.csid,
                path: self.path,
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
            .map(|path_t| MPath::from_thrift(path_t))
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
                    length, csid, path, ..
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
        let (mut ranges, rest) = diff_hunks(parent_content, content).into_iter().fold(
            (Vec::new(), parent_blame.ranges),
            |(mut output, ranges), Hunk { add, remove }| {
                // add uneffected ranges
                let (uneffected, mid) = blame_ranges_split_at(ranges, remove.start as u32);
                output.extend(uneffected);

                // skip removed ranges
                let right = if remove.end > remove.start {
                    let (_removed_blame, right) = blame_ranges_split_at(mid, remove.end as u32);
                    right
                } else {
                    mid
                };

                // add new range
                if add.end > add.start {
                    output.push(BlameRange {
                        // we do not care about offset here since all the ranges at this point
                        // might include incorrect offset. It is fixed lower in the code of this
                        // function.
                        offset: 0,
                        length: (add.end - add.start) as u32,
                        csid,
                        path: path.clone(),
                    });
                }

                (output, right)
            },
        );
        ranges.extend(rest);

        // merge adjacent ranges with identical changeset id and recalculate offsets
        let (_length, ranges): (u32, Vec<BlameRange>) =
            ranges
                .into_iter()
                .fold((0, Vec::new()), |(mut offset, mut output), range| {
                    match output.last_mut() {
                        Some(ref mut last) if last.csid == range.csid => {
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
            result.push_str(&": ");
            result.push_str(line);
            result.push_str("\n");
        }

        Ok(result)
    }
}

/// Split blame ranges at a specified offset
fn blame_ranges_split_at(
    ranges: Vec<BlameRange>,
    offset: u32,
) -> (Vec<BlameRange>, Vec<BlameRange>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    for range in ranges {
        if range.offset + range.length < offset {
            left.push(range);
        } else if offset <= range.offset {
            right.push(range);
        } else {
            let (left_range, right_range) = range.split_at(offset);
            left.extend(left_range);
            right.extend(right_range);
        }
    }
    return (left, right);
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
        |(mut offset, mut output), (csid, path)| -> (u32, Vec<BlameRange>) {
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
    type Item = (ChangesetId, &'a MPath);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.ranges.get(self.ranges_index) {
                None => return None,
                Some(ref range) if self.index < range.offset + range.length => {
                    self.index += 1;
                    return Some((range.csid, &range.path));
                }
                _ => {
                    self.ranges_index += 1;
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
    type Item = (ChangesetId, &'a MPath);

    fn next(&mut self) -> Option<Self::Item> {
        let (last, rest) = self.lines.split_last_mut()?;
        let mut rest = rest.iter_mut();
        while let Some(lines) = rest.next() {
            let (csid, path) = lines.next()?;
            if csid != self.csid {
                // we need to pull all remaining iterators associated with
                // lines with the same index.
                rest.for_each(|lines| {
                    lines.next();
                });
                last.next();
                return Some((csid, path));
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
            },
            BlameRange {
                offset: 2,
                length: 3,
                csid: TWOS_CSID,
                path: p1.clone(),
            },
        ])?;

        let lines: Vec<_> = blame
            .lines()
            .map(|(csid, path)| (csid, path.clone()))
            .collect();
        let reference = vec![
            (ONES_CSID, p0.clone()),
            (ONES_CSID, p0.clone()),
            (TWOS_CSID, p1.clone()),
            (TWOS_CSID, p1.clone()),
            (TWOS_CSID, p1.clone()),
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
        let p1 = MPath::new("path/oen")?;

        let b0 = Blame::new(vec![
            BlameRange {
                offset: 0,
                length: 1,
                csid: TWOS_CSID,
                path: p0.clone(),
            },
            BlameRange {
                offset: 1,
                length: 1,
                csid: FOURS_CSID,
                path: p0.clone(),
            },
            BlameRange {
                offset: 2,
                length: 3,
                csid: ONES_CSID,
                path: p0.clone(),
            },
            BlameRange {
                offset: 5,
                length: 1,
                csid: FOURS_CSID,
                path: p0.clone(),
            },
        ])?;

        let b1 = Blame::new(vec![
            BlameRange {
                offset: 0,
                length: 2,
                csid: FOURS_CSID,
                path: p0.clone(),
            },
            BlameRange {
                offset: 2,
                length: 1,
                csid: ONES_CSID,
                path: p0.clone(),
            },
            BlameRange {
                offset: 3,
                length: 3,
                csid: THREES_CSID,
                path: p1.clone(),
            },
        ])?;

        let reference = Blame::new(vec![
            BlameRange {
                offset: 0,
                length: 1,
                csid: TWOS_CSID,
                path: p0.clone(),
            },
            BlameRange {
                offset: 1,
                length: 1,
                csid: FOURS_CSID,
                path: p0.clone(),
            },
            BlameRange {
                offset: 2,
                length: 1,
                csid: ONES_CSID,
                path: p0.clone(),
            },
            BlameRange {
                offset: 3,
                length: 2,
                csid: THREES_CSID,
                path: p1.clone(),
            },
            BlameRange {
                offset: 5,
                length: 1,
                csid: FOURS_CSID,
                path: p0.clone(),
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
                },
                BlameRange {
                    offset: 1,
                    length: 1,
                    csid: FOURS_CSID,
                    path: p0.clone(),
                },
                BlameRange {
                    offset: 2,
                    length: 1,
                    csid: ONES_CSID,
                    path: p0.clone(),
                },
                BlameRange {
                    offset: 3,
                    length: 2,
                    csid: THREES_CSID,
                    path: p1.clone(),
                },
                BlameRange {
                    offset: 5,
                    length: 1,
                    csid: FOURS_CSID,
                    path: p0.clone(),
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
        }])?;

        assert_eq!(b3_reference, b3);
        Ok(())
    }
}
