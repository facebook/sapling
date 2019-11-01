/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use failure::{err_msg, Error};
use mononoke_types::{ChangesetId, MPath};
use xdiff::{diff_hunks, Hunk};

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BlameRange {
    offset: u32,
    length: u32,
    csid: ChangesetId,
    path: MPath,
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Blame {
    ranges: Vec<BlameRange>,
}

impl Blame {
    pub fn new(ranges: Vec<BlameRange>) -> Result<Self, Error> {
        let mut offset = 0u32;
        for range in ranges.iter() {
            if range.offset != offset {
                return Err(err_msg("ranges could not form valid Blame object"));
            }
            offset += range.length;
        }
        Ok(Blame { ranges })
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

    fn lines<'a>(&'a self) -> BlameLines<'a> {
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
            .ok_or_else(|| err_msg("empty blame for non empty content"))?;
        for (index, line) in content.lines().enumerate() {
            if index as u32 >= range.offset + range.length {
                result.push_str("---------------\n");
                range = ranges
                    .next()
                    .ok_or_else(|| err_msg("not enough ranges in a blame"))?;
            }
            result.push_str(&range.csid.to_string()[..10]);
            result.push_str(" ");
            result.push_str(&format!("{:>4} ", index + 1));
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
struct BlameLines<'a> {
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
    use mononoke_types_mocks::changesetid::{FOURS_CSID, ONES_CSID, THREES_CSID, TWOS_CSID};

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
