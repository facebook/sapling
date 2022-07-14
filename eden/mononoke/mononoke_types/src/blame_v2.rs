/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::blame::BlameRejected;
use crate::path::MPath;
use crate::thrift;
use crate::typed_hash::BlobstoreKey;
use crate::typed_hash::ChangesetId;
use crate::typed_hash::FileUnodeId;
use crate::typed_hash::MononokeId;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bit_set::BitSet;
use blobstore::Blobstore;
use blobstore::BlobstoreBytes;
use blobstore::Loadable;
use blobstore::LoadableError;
use context::CoreContext;
use fbthrift::compact_protocol;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::str::FromStr;
use vec_map::VecMap;
use xdiff::diff_hunks;

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct BlameV2Id(FileUnodeId);

impl BlameV2Id {
    pub fn blobstore_key(&self) -> String {
        format!("blame_v2.{}", self.0.blobstore_key())
    }
    pub fn sampling_fingerprint(&self) -> u64 {
        self.0.sampling_fingerprint()
    }
}

impl FromStr for BlameV2Id {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(BlameV2Id(FileUnodeId::from_str(s)?))
    }
}

impl From<FileUnodeId> for BlameV2Id {
    fn from(file_unode_id: FileUnodeId) -> Self {
        BlameV2Id(file_unode_id)
    }
}

impl From<BlameV2Id> for FileUnodeId {
    fn from(blame_id: BlameV2Id) -> Self {
        blame_id.0
    }
}

impl AsRef<FileUnodeId> for BlameV2Id {
    fn as_ref(&self) -> &FileUnodeId {
        &self.0
    }
}

#[async_trait]
impl Loadable for BlameV2Id {
    type Value = BlameV2;

    async fn load<'a, B: Blobstore>(
        &'a self,
        ctx: &'a CoreContext,
        blobstore: &'a B,
    ) -> Result<Self::Value, LoadableError> {
        let blobstore_key = self.blobstore_key();
        let fetch = blobstore.get(ctx, &blobstore_key);

        let bytes = fetch.await?.ok_or(LoadableError::Missing(blobstore_key))?;
        let blame_t = compact_protocol::deserialize(bytes.as_raw_bytes().as_ref())?;
        let blame = BlameV2::from_thrift(blame_t)?;
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
    blame: BlameV2,
) -> Result<BlameV2Id> {
    let blame_t = blame.into_thrift();
    let data = compact_protocol::serialize(&blame_t);
    let data = BlobstoreBytes::from_bytes(data);
    let blame_id = BlameV2Id::from(file_unode_id);
    blobstore.put(ctx, blame_id.blobstore_key(), data).await?;
    Ok(blame_id)
}

/// Blame data for a particular version of a file.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum BlameV2 {
    /// This version of the file was rejected for blame.
    Rejected(BlameRejected),

    /// This version of the file has blame data.
    Blame(BlameData),
}

impl BlameV2 {
    pub fn new<C: AsRef<[u8]>>(
        csid: ChangesetId,
        path: MPath,
        content: C,
        parents: Vec<BlameParent<C>>,
    ) -> Result<Self> {
        // Filter out parents where the blame data was rejected.  Blame will
        // act as if these parents did not exist.
        let mut parents = parents
            .into_iter()
            .filter_map(BlameParent::into_blame_parent_data);
        if let Some(blame_parent) = parents.next() {
            let mut blame_data =
                BlameData::new_with_parent(csid, path.clone(), &content, blame_parent)?;
            let other_blame_data = parents
                .map(|blame_parent| {
                    BlameData::new_with_parent(csid, path.clone(), &content, blame_parent)
                })
                .collect::<Result<Vec<_>>>()?;
            if !other_blame_data.is_empty() {
                blame_data.merge(csid, &other_blame_data)?;
            }
            blame_data.compact();
            Ok(BlameV2::Blame(blame_data))
        } else {
            Ok(BlameV2::Blame(BlameData::new_root(csid, path, content)))
        }
    }

    pub fn rejected(rejected: BlameRejected) -> Self {
        BlameV2::Rejected(rejected)
    }

    pub fn from_thrift(blame: thrift::BlameV2) -> Result<Self> {
        match blame {
            thrift::BlameV2::rejected(rejected) => {
                Ok(BlameV2::Rejected(BlameRejected::from_thrift(rejected)?))
            }
            thrift::BlameV2::full_blame(blame_data) => {
                Ok(BlameV2::Blame(BlameData::from_thrift(blame_data)?))
            }
            thrift::BlameV2::UnknownField(id) => {
                Err(anyhow!("BlameV2 contains unknown variant with id: {}", id))
            }
        }
    }

    pub fn into_thrift(self) -> thrift::BlameV2 {
        match self {
            BlameV2::Blame(blame_data) => thrift::BlameV2::full_blame(blame_data.into_thrift()),
            BlameV2::Rejected(rejected) => thrift::BlameV2::rejected(rejected.into_thrift()),
        }
    }

    pub fn ranges(&self) -> Result<BlameRanges<'_>, BlameRejected> {
        match self {
            BlameV2::Blame(blame_data) => Ok(BlameRanges::new(blame_data)),
            BlameV2::Rejected(rejected) => Err(rejected.clone()),
        }
    }

    pub fn lines(&self) -> Result<BlameLines<'_>, BlameRejected> {
        match self {
            BlameV2::Blame(blame_data) => Ok(BlameLines::new(blame_data)),
            BlameV2::Rejected(rejected) => Err(rejected.clone()),
        }
    }

    pub fn changeset_ids(
        &self,
    ) -> Result<impl Iterator<Item = (ChangesetId, u32)> + '_, BlameRejected> {
        match self {
            BlameV2::Blame(blame_data) => Ok(blame_data
                .csids
                .iter()
                .map(|(number, csid)| (*csid, number as u32))),
            BlameV2::Rejected(rejected) => Err(rejected.clone()),
        }
    }

    pub fn annotate(&self, content: &str) -> Result<String> {
        match self {
            BlameV2::Blame(blame_data) => blame_data.annotate(content),
            BlameV2::Rejected(rejected) => Err(rejected.clone().into()),
        }
    }

    pub fn apply_mutable_change(
        &mut self,
        original_ancestor: &Self,
        mutated_ancestor: &Self,
    ) -> Result<()> {
        match (self, original_ancestor, mutated_ancestor) {
            (BlameV2::Rejected(_), _, _) => {
                // No blame, so pass on unchanged
            }
            (_, BlameV2::Rejected(_), BlameV2::Rejected(_)) => {
                // Both old and new blame are rejected, so nothing to fix
            }
            (_, BlameV2::Rejected(reason), _) | (_, _, BlameV2::Rejected(reason)) => {
                // Blame rejection happens based on the file at this point
                // As rejection cannot be due to the *parents*, this is an impossible case.
                bail!(
                    "Ancestor blame is inconsistently rejected ({:?} for the reject, but other form is not rejected) - this should not be possible.",
                    reason
                );
            }
            (
                BlameV2::Blame(self_blame),
                BlameV2::Blame(original_blame),
                BlameV2::Blame(mutated_blame),
            ) => {
                self_blame.apply_mutable_change(original_blame, mutated_blame)?;
            }
        }
        Ok(())
    }
}

/// Blame for a parent file version when constructing a new blame.
pub struct BlameParent<C: AsRef<[u8]>> {
    parent_index: usize,
    path: MPath,
    content: Option<C>,
    blame: BlameV2,
}

impl<C: AsRef<[u8]>> BlameParent<C> {
    pub fn new(
        parent_index: usize,
        path: MPath,
        content: impl Into<Option<C>>,
        blame: BlameV2,
    ) -> BlameParent<C> {
        let content = content.into();
        BlameParent {
            parent_index,
            path,
            content,
            blame,
        }
    }

    fn into_blame_parent_data(self) -> Option<BlameParentData<C>> {
        match (self.blame, self.content) {
            (BlameV2::Blame(blame), Some(content)) => Some(BlameParentData {
                parent_index: self.parent_index as u32,
                path: self.path,
                content,
                blame,
            }),
            _ => None,
        }
    }
}

/// Blame data for a parent file version when constructing a new blame.
pub struct BlameParentData<C: AsRef<[u8]>> {
    parent_index: u32,
    path: MPath,
    content: C,
    blame: BlameData,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BlameData {
    /// Sequence of index-based blame ranges for this file.
    ranges: Vec<BlameRangeIndexes>,

    /// A map of changeset_id index to changeset ID.  The keys of this map
    /// are stable for the p1-parent history of a changeset.  A `VecMap`
    /// is used so that look-up is O(1) and the keys can be traversed
    /// in-order.
    csids: VecMap<ChangesetId>,

    /// The maximum index in `csids`.  This is the changeset_id index of
    /// the current changeset.
    max_csid_index: u32,

    /// A list of all paths this file has ever been located at.  Used
    /// as an index for paths in `ranges`.
    paths: Vec<MPath>,
}

impl BlameData {
    /// Create a new BlameData for a brand new file.
    fn new_root<C: AsRef<[u8]>>(csid: ChangesetId, path: MPath, content: C) -> Self {
        let mut ranges = Vec::new();
        let mut csids = VecMap::new();
        // Calculate length by diffing with empty content, so the number of lines
        // is calculated the same way as xdiff does.  If `diff_hunks` yields
        // no hunks then the file is empty and the list of ranges should also
        // be empty.
        if let Some(hunk) = diff_hunks(&b""[..], content.as_ref()).first() {
            let length = (hunk.add.end - hunk.add.start) as u32;
            debug_assert!(length > 0);
            ranges.push(BlameRangeIndexes {
                offset: 0,
                length,
                csid_index: 0,
                path_index: 0,
                origin_offset: 0,
                parent: None,
            });
            csids.insert(0, csid);
        };
        // The blame data always contains all paths the file has ever been located at,
        // and the csid.  The csids map only contains the changeset ID if the
        // file is not empty and some lines have been added.  This means empty
        // files are an edge case where csid_index 0 has been assigned to this
        // changeset but since there are no lines attributed to it, it is
        // stripped from the csids map, the same as if `compact` was called.
        BlameData {
            ranges,
            csids,
            max_csid_index: 0,
            paths: vec![path],
        }
    }

    /// Create a new BlameData for a file with a single parent.
    fn new_with_parent<C: AsRef<[u8]>>(
        csid: ChangesetId,
        path: MPath,
        content: &C,
        blame_parent: BlameParentData<C>,
    ) -> Result<Self> {
        // Assign a new index for the changeset ID.  The changeset ID
        // shouldn't be present already, as this should be a child changeset.
        let mut csids = blame_parent.blame.csids.clone();
        if csids.values().any(|existing_csid| &csid == existing_csid) {
            return Err(anyhow!(
                "{} already exists in the history of this blame.",
                csid,
            ));
        }
        let csid_index = blame_parent.blame.max_csid_index + 1;
        csids.insert(csid_index as usize, csid);

        let mut paths = blame_parent.blame.paths.clone();

        let renamed_from_path_index = if blame_parent.path != path {
            match paths.iter().rposition(|p| &blame_parent.path == p) {
                Some(index) => Some(index as u32),
                None => {
                    return Err(anyhow!(
                        "parent path '{}' is not known to blame data",
                        blame_parent.path
                    ));
                }
            }
        } else {
            None
        };

        let path_index = match paths.iter().rposition(|p| &path == p) {
            Some(index) => index as u32,
            None => {
                let path_index = paths.len() as u32;
                paths.push(path);
                path_index
            }
        };

        // Hunks coming from `diff_hunks` have two associated ranges: `add`
        // and `remove`.  Each hunk always talks about the same place in the
        // code (we are replacing `remove` with `add`).  For each hunk, add a
        // new range corresponding to `add` after removing the range from
        // `remove`.
        let mut new_ranges = BlameRangesCollector::new();
        let mut parent_ranges = VecDeque::from(blame_parent.blame.ranges);
        for hunk in diff_hunks(blame_parent.content.as_ref(), content.as_ref()) {
            // Add unaffected ranges
            let (unaffected, remaining) =
                BlameRangeIndexes::split_multiple_at(parent_ranges, hunk.remove.start as u32);
            parent_ranges = remaining;
            for range in unaffected {
                new_ranges.append(range);
            }

            // Skip the removed ranges
            if hunk.remove.end > hunk.remove.start {
                let (_removed_ranges, remaining) =
                    BlameRangeIndexes::split_multiple_at(parent_ranges, hunk.remove.end as u32);
                parent_ranges = remaining;
            }

            // Add a new range
            if hunk.add.end > hunk.add.start {
                let length = (hunk.add.end - hunk.add.start) as u32;
                let parent = Some(BlameParentIndexes {
                    parent_index: blame_parent.parent_index,
                    offset: hunk.remove.start as u32,
                    length: (hunk.remove.end - hunk.remove.start) as u32,
                    renamed_from_path_index,
                });
                new_ranges.append_new(csid_index, path_index, length, parent);
            }
        }

        // Take all the left-over ranges as-is.
        for range in parent_ranges {
            new_ranges.append(range);
        }

        // Merge adjacent ranges with identical changeset id and matching
        // origin offsets.  We must check that origin offsets match so that
        // when lines are deleted from the middle of a range, the ranges
        // either side still refer to the correct original lines.
        let mut ranges: Vec<BlameRangeIndexes> = Vec::with_capacity(new_ranges.len());
        for range in new_ranges.take() {
            match ranges.last_mut() {
                None if range.offset != 0 => {
                    return Err(anyhow!(
                        "programming error: non-zero initial offset: {}",
                        range.offset
                    ));
                }
                Some(last) if range.offset != last.offset + last.length => {
                    return Err(anyhow!(
                        "programming error: discontinuous offsets: {} + {} and {}",
                        last.offset,
                        last.length,
                        range.offset
                    ));
                }
                Some(last)
                    if last.csid_index == range.csid_index
                        && last.origin_offset + last.length == range.origin_offset
                        && last.parent == range.parent =>
                {
                    last.length += range.length;
                }
                _ => ranges.push(range),
            }
        }

        Ok(BlameData {
            ranges,
            csids,
            max_csid_index: csid_index,
            paths,
        })
    }

    /// Construct a merge iterator that merges this blame data with the data
    /// from others.
    fn merge_lines<'a>(
        &'a self,
        merge_csid: ChangesetId,
        others: &'a [BlameData],
    ) -> BlameMergeLines<'a> {
        BlameMergeLines::new(
            merge_csid,
            Some(self)
                .into_iter()
                .chain(others)
                .map(BlameLines::new)
                .collect(),
        )
    }

    /// Merge other blame data into this blame for a merge changeset.
    fn merge(&mut self, merge_csid: ChangesetId, others: &[BlameData]) -> Result<()> {
        // Remove the csid index for the merge changeset; we will re-assign it
        // once all other blames have been merged in.
        let mut next_csid_index = self.max_csid_index;
        let mut merged_csids = self.csids.clone();
        merged_csids.remove(next_csid_index as usize);

        // Build reverse indexes for csids and paths.
        let mut csid_indexes: HashMap<ChangesetId, u32> = merged_csids
            .iter()
            .map(|(index, csid)| (*csid, index as u32))
            .collect();
        let mut path_indexes: HashMap<MPath, u32> = self
            .paths
            .iter()
            .enumerate()
            .map(|(index, path)| (path.clone(), index as u32))
            .collect();

        // Make a first pass across the merged blame data to determine the set
        // of changesets and paths that need to be added.
        let mut new_csids = HashSet::new();
        let mut new_paths = HashSet::new();
        for blame_line in self.merge_lines(merge_csid, others) {
            if blame_line.changeset_id != &merge_csid
                && !csid_indexes.contains_key(blame_line.changeset_id)
            {
                new_csids.insert(*blame_line.changeset_id);
            }
            if !path_indexes.contains_key(blame_line.path) && !new_paths.contains(blame_line.path) {
                new_paths.insert(blame_line.path.clone());
            }
        }

        // Assign indexes to the new changesets and paths, in the order they
        // appear in the merged blame data.
        for other in others {
            for other_csid in other.csids.values() {
                if new_csids.remove(other_csid) {
                    merged_csids.insert(next_csid_index as usize, *other_csid);
                    csid_indexes.insert(*other_csid, next_csid_index);
                    next_csid_index += 1;
                }
            }
            for other_path in other.paths.iter() {
                if new_paths.remove(other_path) {
                    path_indexes.insert(other_path.clone(), self.paths.len() as u32);
                    self.paths.push(other_path.clone());
                }
            }
        }

        // The csid index for the merge changeset is the next index that is
        // free.
        merged_csids.insert(next_csid_index as usize, merge_csid);
        csid_indexes.insert(merge_csid, next_csid_index);

        // Make a second pass across the merged lines and collect into ranges.
        // We must check that origin offsets match so that when lines are
        // deleted from the middle of a range, the ranges either side still
        // refer to the correct original lines.
        let mut merged_ranges: Vec<BlameRangeIndexes> = Vec::new();
        for blame_line in self.merge_lines(merge_csid, others) {
            let path_index = path_indexes[blame_line.path];
            let csid_index = csid_indexes[blame_line.changeset_id];
            let parent = blame_line.parent(&path_indexes);
            match merged_ranges.last_mut() {
                None if blame_line.offset != 0 => {
                    return Err(anyhow!(
                        "programming error: non-zero initial offset: {}",
                        blame_line.offset
                    ));
                }
                Some(last) if blame_line.offset != last.offset + last.length => {
                    return Err(anyhow!(
                        "programming error: discontinuous offsets: {} + {} and {}",
                        last.offset,
                        last.length,
                        blame_line.offset
                    ));
                }
                Some(last)
                    if last.csid_index == csid_index
                        && last.path_index == path_index
                        && last.origin_offset + last.length == blame_line.origin_offset
                        && last.parent == parent =>
                {
                    last.length += 1;
                }
                _ => {
                    merged_ranges.push(BlameRangeIndexes {
                        offset: blame_line.offset,
                        length: 1,
                        csid_index,
                        path_index,
                        origin_offset: blame_line.origin_offset,
                        parent,
                    });
                }
            }
        }

        self.ranges = merged_ranges;
        self.csids = merged_csids;
        self.max_csid_index = next_csid_index;

        Ok(())
    }

    /// Remove unreferenced changeset ids.
    fn compact(&mut self) {
        let mut seen_csid_indexes = BitSet::with_capacity(self.max_csid_index as usize + 1);
        for range in self.ranges.iter() {
            seen_csid_indexes.insert(range.csid_index as usize);
        }
        self.csids
            .retain(|index, _| seen_csid_indexes.contains(index as usize));
    }

    fn from_thrift(blame: thrift::BlameDataV2) -> Result<BlameData> {
        let paths = blame
            .paths
            .into_iter()
            .map(MPath::from_thrift)
            .collect::<Result<Vec<_>, _>>()?;
        let mut csids = VecMap::with_capacity(blame.max_csid_index.0 as usize + 1);
        for (index, csid) in blame.csids {
            csids.insert(index as usize, ChangesetId::from_thrift(csid)?);
        }
        let mut ranges = Vec::with_capacity(blame.ranges.len());
        let mut offset = 0;
        for range in blame.ranges {
            let length = range.length as u32;
            let csid_index = range.csid_index.0 as u32;
            if !csids.contains_key(csid_index as usize) {
                return Err(anyhow!(
                    "invalid blame changeset index for range at {}: {}",
                    offset,
                    csid_index
                ));
            }
            let path_index = range.path_index.0 as u32;
            if path_index as usize >= paths.len() {
                return Err(anyhow!(
                    "invalid blame path index for range at {}: {}",
                    offset,
                    path_index
                ));
            }
            let origin_offset = range.origin_offset as u32;
            let parent = if let (Some(parent_offset), Some(parent_length)) =
                (range.parent_offset, range.parent_length)
            {
                Some(BlameParentIndexes {
                    offset: parent_offset as u32,
                    length: parent_length as u32,
                    parent_index: range.parent_index.unwrap_or(0) as u32,
                    renamed_from_path_index: range.renamed_from_path_index.map(|i| i.0 as u32),
                })
            } else {
                None
            };
            ranges.push(BlameRangeIndexes {
                offset,
                length,
                csid_index,
                path_index,
                origin_offset,
                parent,
            });
            offset += length;
        }
        Ok(BlameData {
            ranges,
            csids,
            max_csid_index: blame.max_csid_index.0 as u32,
            paths,
        })
    }

    fn into_thrift(self) -> thrift::BlameDataV2 {
        let ranges = self
            .ranges
            .into_iter()
            .map(|range| thrift::BlameRangeV2 {
                length: range.length as i32,
                csid_index: thrift::BlameChangeset(range.csid_index as i32),
                path_index: thrift::BlamePath(range.path_index as i32),
                origin_offset: range.origin_offset as i32,
                parent_offset: range.parent.as_ref().map(|p| p.offset as i32),
                parent_length: range.parent.as_ref().map(|p| p.length as i32),
                renamed_from_path_index: range.parent.as_ref().and_then(|p| {
                    p.renamed_from_path_index
                        .map(|i| thrift::BlamePath(i as i32))
                }),
                parent_index: range.parent.as_ref().and_then(|p| {
                    if p.parent_index != 0 {
                        Some(p.parent_index as i32)
                    } else {
                        None
                    }
                }),
            })
            .collect();
        let csids = self
            .csids
            .into_iter()
            .map(|(index, csid)| (index as i32, csid.into_thrift()))
            .collect();
        let max_csid_index = thrift::BlameChangeset(self.max_csid_index as i32);
        let paths = self.paths.into_iter().map(MPath::into_thrift).collect();

        thrift::BlameDataV2 {
            ranges,
            csids,
            max_csid_index,
            paths,
        }
    }

    /// Generate a string containing content annotated with this blame data.
    fn annotate(&self, content: &str) -> Result<String> {
        if content.is_empty() {
            return Ok(String::new());
        }

        let mut result = String::new();
        let mut ranges = self.ranges.iter();
        let mut range = ranges
            .next()
            .ok_or_else(|| Error::msg("empty blame for non empty content"))?;
        let mut origin_offset = range.origin_offset;
        for (index, line) in content.lines().enumerate() {
            if index as u32 >= range.offset + range.length {
                range = ranges
                    .next()
                    .ok_or_else(|| Error::msg("not enough ranges in blame"))?;
                origin_offset = range.origin_offset;
            }
            let csid = self.csids[range.csid_index as usize];
            result.push_str(&csid.to_string()[..12]);
            result.push_str(&format!(":{}: ", origin_offset + 1));
            result.push_str(line);
            result.push('\n');
            origin_offset += 1;
        }

        Ok(result)
    }

    fn apply_mutable_change(&mut self, original_blame: &Self, mutated_blame: &Self) -> Result<()> {
        // Sort order of changesets is not guaranteed to be the same, which rules out Iterator::eq
        let my_csids: HashSet<_> = self.csids.values().collect();

        if original_blame
            .csids
            .values()
            .any(|csid| my_csids.contains(&csid))
        {
            // Track seen hashes - we can assume that we'll see about as many as
            // are in the immutable blame
            let mut seen_hashes = HashSet::with_capacity(my_csids.capacity());
            // Paths is mutant paths, followed by any missing paths from self
            // As self currently covers original, this will be a superset of paths in the commit
            let new_paths = {
                let mut new_paths = mutated_blame.paths.clone();
                new_paths.extend(
                    self.paths
                        .iter()
                        .filter(|path| !mutated_blame.paths.contains(path))
                        .cloned(),
                );
                new_paths
            };

            // Reblame line-by-line in terms of changeset hashes only.
            let new_lines: Vec<_> = {
                let mut mutation_lookup: HashMap<_, _> = BlameLines::new(original_blame)
                    .map(|line| (line.changeset_id, line.path, line.origin_offset))
                    .zip(BlameLines::new(mutated_blame))
                    .collect();
                BlameLines::new(self)
                    .map(|line| {
                        let key = (line.changeset_id, line.path, line.origin_offset);
                        let out = mutation_lookup.remove(&key).unwrap_or(line);
                        seen_hashes.insert(*out.changeset_id);
                        out
                    })
                    .collect()
            };

            let (new_csids, new_max_csid_index) = {
                // Copy the list from the mutant blame - that's our deep history
                let mut new_csids = mutated_blame.csids.clone();
                let mut new_max_csid_index = mutated_blame.max_csid_index as usize;
                // Add anything that hasn't disappered from our original history
                // This will be merge cases, where we've kept the old index, but have
                // blamed to a commit *after* the mutation point
                let mut known_so_far: HashSet<_> = new_csids.values().copied().collect();
                for original_csid in original_blame.csids.values() {
                    if seen_hashes.contains(original_csid) {
                        if known_so_far.insert(*original_csid) {
                            new_max_csid_index += 1;
                            new_csids.insert(new_max_csid_index, *original_csid);
                        }
                    }
                }
                let original_csids: HashSet<_> = original_blame.csids.values().collect();
                // And add in all the csids from the current blame that weren't in the pre-mutation point
                // blame - these are all csids we'd have if we did a real blame from scratch
                for csid in self
                    .csids
                    .values()
                    .filter(|csid| !original_csids.contains(csid))
                {
                    if known_so_far.insert(*csid) {
                        new_max_csid_index += 1;
                        new_csids.insert(new_max_csid_index, *csid);
                    }
                }
                (
                    new_csids,
                    new_max_csid_index
                        .try_into()
                        .context("More than 2**32 changesets in this blame!")?,
                )
            };

            // Create a reverse map from new_csids
            let csid_to_index: HashMap<_, u32> = new_csids
                .iter()
                .map(|(index, csid)| (csid, index as u32))
                .collect();
            // And one for paths
            let path_to_index: HashMap<_, u32> = new_paths
                .iter()
                .enumerate()
                .map(|(index, path)| (path, index as u32))
                .collect();

            let line_follows_range = |line: &BlameLine<'_>, range: &BlameRangeIndexes| -> bool {
                let range_path_index: usize = range.path_index as usize;
                let maybe_range_path: Option<&MPath> = new_paths.get(range_path_index);
                Some(line.changeset_id) == new_csids.get(range.csid_index as usize)
                    && Some(line.path) == maybe_range_path
                    && line.offset == range.offset + range.length
                    && line.origin_offset == range.origin_offset + range.length
                    && line.parent.zip(range.parent.as_ref()).map_or(
                        true,
                        |(line_parent, range_parent)| {
                            let maybe_range_path =
                                range_parent.renamed_from_path_index.and_then(|index| {
                                    let index: usize = index as usize;
                                    new_paths.get(index)
                                });
                            line_parent.parent_index == range_parent.parent_index
                                && line_parent.offset == range_parent.offset
                                && line_parent.length == range_parent.length
                                && line_parent.renamed_from_path == maybe_range_path
                        },
                    )
            };
            // Now can go over new_lines, building blame ranges
            let new_ranges = {
                let (mut new_ranges, last_range) = new_lines.into_iter().enumerate().try_fold(
                    (Vec::new(), None),
                    |(mut out, range), (offset, line)| {
                        if let Some(mut range) = range {
                            if line_follows_range(&line, &range) {
                                range.length += 1;
                                return Ok((out, Some(range)));
                            } else {
                                out.push(range);
                            }
                        }

                        let csid_index =
                            *csid_to_index.get(line.changeset_id).with_context(|| {
                                format!(
                                    "Unknown changeset {} - should not be possible",
                                    line.changeset_id
                                )
                            })?;
                        let path_index = *path_to_index.get(line.path).with_context(|| {
                            format!("Unknown path {} - should not be possible", line.path)
                        })?;
                        let offset = offset as u32;
                        let parent = line
                            .parent
                            .map(
                                |BlameLineParent {
                                     parent_index,
                                     offset,
                                     length,
                                     renamed_from_path,
                                 }| {
                                    let renamed_from_path_index = renamed_from_path
                                        .map(|renamed_from_path| {
                                            path_to_index.get(renamed_from_path).with_context(
                                                || {
                                                    format!(
                                                        "Unknown path {} - should not be possible",
                                                        renamed_from_path
                                                    )
                                                },
                                            )
                                        })
                                        .transpose()?
                                        .copied();
                                    anyhow::Ok(BlameParentIndexes {
                                        parent_index,
                                        offset,
                                        length,
                                        renamed_from_path_index,
                                    })
                                },
                            )
                            .transpose()?;

                        let range = BlameRangeIndexes {
                            offset,
                            length: 1,
                            csid_index,
                            path_index,
                            origin_offset: line.origin_offset,
                            parent,
                        };
                        anyhow::Ok((out, Some(range)))
                    },
                )?;
                if let Some(last_range) = last_range {
                    new_ranges.push(last_range);
                }
                new_ranges
            };
            // Then rewrite our object
            self.ranges = new_ranges;
            self.csids = new_csids;
            self.max_csid_index = new_max_csid_index;
            self.paths = new_paths;
            self.compact()
        }

        Ok(())
    }
}

/// Blame range with range information stored as indexes into the associated
/// look-up tables.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BlameRangeIndexes {
    pub offset: u32,
    pub length: u32,
    pub csid_index: u32,
    pub path_index: u32,
    pub origin_offset: u32,
    pub parent: Option<BlameParentIndexes>,
}

impl BlameRangeIndexes {
    fn split_at(self, offset: u32) -> (Option<BlameRangeIndexes>, Option<BlameRangeIndexes>) {
        if offset <= self.offset {
            (None, Some(self))
        } else if offset >= self.offset + self.length {
            (Some(self), None)
        } else {
            let left = BlameRangeIndexes {
                offset: self.offset,
                length: offset - self.offset,
                csid_index: self.csid_index,
                path_index: self.path_index,
                origin_offset: self.origin_offset,
                parent: self.parent.clone(),
            };
            let right = BlameRangeIndexes {
                offset,
                length: self.length - left.length,
                csid_index: self.csid_index,
                path_index: self.path_index,
                origin_offset: self.origin_offset + left.length,
                parent: self.parent,
            };
            (Some(left), Some(right))
        }
    }

    /// Split a sequence of ranges at a given offset.
    fn split_multiple_at(
        mut ranges: VecDeque<BlameRangeIndexes>,
        offset: u32,
    ) -> (VecDeque<BlameRangeIndexes>, VecDeque<BlameRangeIndexes>) {
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
}

/// Parent of a blame range - the range that was replaced.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct BlameParentIndexes {
    /// Index of the parent in the bonsai changeset.
    pub parent_index: u32,

    /// Offset of the replaced range.
    pub offset: u32,

    /// Length of the replaced range.
    pub length: u32,

    /// If the file was renamed, index into "paths" of the old filename.
    pub renamed_from_path_index: Option<u32>,
}

/// Struct to collect a new set of blame ranges while maintaining
/// the offsets correctly.
struct BlameRangesCollector {
    ranges: Vec<BlameRangeIndexes>,
    offset: u32,
}

impl BlameRangesCollector {
    fn new() -> Self {
        BlameRangesCollector {
            ranges: Vec::new(),
            offset: 0,
        }
    }

    /// Append an existing range to the set of ranges.  The range offset is
    /// updated.
    fn append(&mut self, mut range: BlameRangeIndexes) {
        range.offset = self.offset;
        self.offset += range.length;
        self.ranges.push(range);
    }

    /// Append a new range to the set of ranges.  The range offset is
    /// determined automatically.
    fn append_new(
        &mut self,
        csid_index: u32,
        path_index: u32,
        length: u32,
        parent: Option<BlameParentIndexes>,
    ) {
        self.ranges.push(BlameRangeIndexes {
            offset: self.offset,
            length,
            csid_index,
            path_index,
            origin_offset: self.offset,
            parent,
        });
        self.offset += length;
    }

    fn len(&self) -> usize {
        self.ranges.len()
    }

    fn take(self) -> Vec<BlameRangeIndexes> {
        self.ranges
    }
}

/// Blame range produced by iteration.
#[derive(PartialEq)]
pub struct BlameRange<'a> {
    pub offset: u32,
    pub length: u32,
    pub csid: ChangesetId,
    pub path: &'a MPath,
    pub origin_offset: u32,
}

pub struct BlameRanges<'a> {
    data: &'a BlameData,
    range_index: usize,
}

impl<'a> BlameRanges<'a> {
    fn new(data: &'a BlameData) -> BlameRanges<'a> {
        BlameRanges {
            data,
            range_index: 0,
        }
    }
}

impl<'a> Iterator for BlameRanges<'a> {
    type Item = BlameRange<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.data.ranges.get(self.range_index).map(|range| {
            self.range_index += 1;
            BlameRange {
                offset: range.offset,
                length: range.length,
                csid: self.data.csids[range.csid_index as usize],
                path: &self.data.paths[range.path_index as usize],
                origin_offset: range.origin_offset,
            }
        })
    }
}

/// Iterator over blame data as if it was just a list of lines with associated
/// changeset id and path.
#[derive(Clone)]
pub struct BlameLines<'a> {
    data: &'a BlameData,
    range_index: usize,
    range_offset: u32,
}

impl<'a> BlameLines<'a> {
    fn new(data: &'a BlameData) -> BlameLines<'a> {
        BlameLines {
            data,
            range_index: 0,
            range_offset: 0,
        }
    }
}

#[derive(Copy, Clone)]
pub struct BlameLineParent<'a> {
    pub parent_index: u32,
    pub offset: u32,
    pub length: u32,
    pub renamed_from_path: Option<&'a MPath>,
}

/// Blame line produced by iteration.
pub struct BlameLine<'a> {
    pub offset: u32,
    pub changeset_index: u32,
    pub changeset_id: &'a ChangesetId,
    pub path_index: u32,
    pub path: &'a MPath,
    pub origin_offset: u32,
    pub parent: Option<BlameLineParent<'a>>,
}

impl<'a> BlameLine<'a> {
    fn new(data: &'a BlameData, range: &BlameRangeIndexes, range_offset: u32) -> Self {
        let parent = range.parent.as_ref().map(|parent| BlameLineParent {
            parent_index: parent.parent_index,
            offset: parent.offset,
            length: parent.length,
            renamed_from_path: parent
                .renamed_from_path_index
                .map(|i| &data.paths[i as usize]),
        });
        BlameLine {
            offset: range.offset + range_offset,
            changeset_index: range.csid_index,
            changeset_id: &data.csids[range.csid_index as usize],
            path_index: range.path_index,
            path: &data.paths[range.path_index as usize],
            origin_offset: range.origin_offset + range_offset,
            parent,
        }
    }

    fn parent(&self, path_indexes: &HashMap<MPath, u32>) -> Option<BlameParentIndexes> {
        self.parent.as_ref().map(|parent| BlameParentIndexes {
            parent_index: parent.parent_index,
            offset: parent.offset,
            length: parent.length,
            renamed_from_path_index: parent.renamed_from_path.map(|p| path_indexes[p]),
        })
    }
}

impl<'a> Iterator for BlameLines<'a> {
    type Item = BlameLine<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.data.ranges.get(self.range_index) {
                None => return None,
                Some(range) if self.range_offset < range.length => {
                    let line = BlameLine::new(self.data, range, self.range_offset);
                    self.range_offset += 1;
                    return Some(line);
                }
                _ => {
                    self.range_index += 1;
                    self.range_offset = 0;
                }
            }
        }
    }
}

/// Merge iterator on a list of `BlameLines` iterators.
///
/// This iterator merges together a list of `BlameLines` iterators, one per
/// parent of a merge commit.  For each line in the file, it selects the blame
/// line for the *first* parent that blames the line on itself or one of its
/// ancestors.  If none of the parents blame the line on themselves or their
/// ancestors, then the line was added in the merge commit and will be blamed
/// on the merge commit.
#[derive(Clone)]
struct BlameMergeLines<'a> {
    /// The merge changeset that this iterator was created for.
    csid: ChangesetId,

    /// A list of lines iterators for each of the merge sources.  These
    /// iterators are advanced in lock-step, so they always refer to the
    /// same line in the file.
    lines: Vec<BlameLines<'a>>,
}

impl<'a> BlameMergeLines<'a> {
    fn new(csid: ChangesetId, lines: Vec<BlameLines<'a>>) -> Self {
        Self { csid, lines }
    }
}

impl<'a> Iterator for BlameMergeLines<'a> {
    type Item = BlameLine<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        // Pick a `BlameLine` for the next merged line.  For each of the
        // values yielded from the `lines` iterators, we take the first one
        // that does *not* match the merge changeset.  This give us the first
        // parent which blames the line on itself or one of its ancestors.
        let mut first_blame_line = None;
        let mut rest = self.lines.iter_mut();
        while let Some(lines) = rest.next() {
            let blame_line = lines.next()?;
            if blame_line.changeset_id != &self.csid {
                // This line is blamed on this parent or one of its ancestors.
                // Use its `BlameLine` for this line in the file.
                //
                // We're done with this line, but we still need to pull all of
                // the remaining iterators to ensure that all of them have
                // moved to the next line.
                for lines in rest {
                    lines.next();
                }
                return Some(blame_line);
            }
            // If this is the first parent, save it so that we can return its
            // blame line if all the others fail.
            if first_blame_line.is_none() {
                first_blame_line = Some(blame_line);
            }
        }
        // None of the parents blame this line on an ancestor.  Take the
        // value from the first parent as-is, which will blame it on the merge
        // commit.
        first_blame_line
    }
}

#[cfg(test)]
mod test {
    #![allow(clippy::redundant_clone)]

    use super::*;
    use crate::hash::Blake2;
    use pretty_assertions::assert_eq;

    const ONES_CSID: ChangesetId = ChangesetId::new(Blake2::from_byte_array([0x11; 32]));
    const TWOS_CSID: ChangesetId = ChangesetId::new(Blake2::from_byte_array([0x22; 32]));
    const THREES_CSID: ChangesetId = ChangesetId::new(Blake2::from_byte_array([0x33; 32]));
    const FOURS_CSID: ChangesetId = ChangesetId::new(Blake2::from_byte_array([0x44; 32]));
    const FIVES_CSID: ChangesetId = ChangesetId::new(Blake2::from_byte_array([0x55; 32]));
    const SIXES_CSID: ChangesetId = ChangesetId::new(Blake2::from_byte_array([0x66; 32]));
    const SEVENS_CSID: ChangesetId = ChangesetId::new(Blake2::from_byte_array([0x77; 32]));

    macro_rules! vec_map {
        () => {
            VecMap::new()
        };
        ( $( $k:expr => $v:expr ),* $(,)? ) => {
            {
                let mut v = VecMap::new();
                $( v.insert($k, $v); )*
                v
            }
        };
    }

    #[test]
    fn test_thrift() -> Result<()> {
        let p0 = MPath::new("path/zero")?;
        let p1 = MPath::new("path/one")?;

        let mut csids = VecMap::new();
        csids.insert(0, ONES_CSID);
        csids.insert(1, TWOS_CSID);
        csids.insert(3, THREES_CSID);
        csids.insert(4, FOURS_CSID);

        let blame = BlameV2::Blame(BlameData {
            ranges: vec![
                BlameRangeIndexes {
                    offset: 0,
                    length: 1,
                    csid_index: 1,
                    path_index: 0,
                    origin_offset: 5,
                    parent: None,
                },
                BlameRangeIndexes {
                    offset: 1,
                    length: 1,
                    csid_index: 4,
                    path_index: 0,
                    origin_offset: 31,
                    parent: None,
                },
                BlameRangeIndexes {
                    offset: 2,
                    length: 1,
                    csid_index: 0,
                    path_index: 0,
                    origin_offset: 127,
                    parent: None,
                },
                BlameRangeIndexes {
                    offset: 3,
                    length: 2,
                    csid_index: 3,
                    path_index: 1,
                    origin_offset: 15,
                    parent: None,
                },
                BlameRangeIndexes {
                    offset: 5,
                    length: 1,
                    csid_index: 4,
                    path_index: 0,
                    origin_offset: 3,
                    parent: None,
                },
            ],
            csids,
            max_csid_index: 4,
            paths: vec![p0.clone(), p1.clone()],
        });

        let blame_thrift = blame.clone().into_thrift();
        assert_eq!(BlameV2::from_thrift(blame_thrift)?, blame);

        Ok(())
    }

    #[test]
    fn test_annotate() -> Result<()> {
        let mut csids = VecMap::new();
        csids.insert(0, ONES_CSID);
        csids.insert(1, TWOS_CSID);
        csids.insert(3, THREES_CSID);
        csids.insert(4, FOURS_CSID);

        let blame = BlameV2::Blame(BlameData {
            ranges: vec![
                BlameRangeIndexes {
                    offset: 0,
                    length: 1,
                    csid_index: 1,
                    path_index: 0,
                    origin_offset: 5,
                    parent: None,
                },
                BlameRangeIndexes {
                    offset: 1,
                    length: 1,
                    csid_index: 3,
                    path_index: 0,
                    origin_offset: 2,
                    parent: None,
                },
                BlameRangeIndexes {
                    offset: 2,
                    length: 2,
                    csid_index: 4,
                    path_index: 0,
                    origin_offset: 1,
                    parent: None,
                },
                BlameRangeIndexes {
                    offset: 3,
                    length: 1,
                    csid_index: 0,
                    path_index: 0,
                    origin_offset: 0,
                    parent: None,
                },
            ],
            csids,
            max_csid_index: 4,
            paths: vec![MPath::new("file")?],
        });

        assert_eq!(
            blame.annotate("one\ntwo\nthree\nfour\nfive\n")?,
            concat!(
                "222222222222:6: one\n",
                "333333333333:3: two\n",
                "444444444444:2: three\n",
                "444444444444:3: four\n",
                "111111111111:1: five\n",
            )
        );

        Ok(())
    }

    #[test]
    fn test_linear() -> Result<()> {
        let path1 = MPath::new("path")?;
        let path2 = MPath::new("new/path")?;

        let c1 = "one\ntwo\nthree\nfour\n";
        let c2 = "one\nfive\nsix\nfour\n";
        let c3 = "seven\none\nsix\neight\nfour\n";
        let c4 = "seven\none\nnine\nten\neight\nfour\n";
        let c5 = "one\n";

        let b1 = BlameV2::new(ONES_CSID, path1.clone(), c1, vec![])?;
        let b2 = BlameV2::new(
            TWOS_CSID,
            path1.clone(),
            c2,
            vec![BlameParent::new(0, path1.clone(), c1, b1.clone())],
        )?;
        let b3 = BlameV2::new(
            THREES_CSID,
            path1.clone(),
            c3,
            vec![BlameParent::new(0, path1.clone(), c2, b2.clone())],
        )?;
        let b4 = BlameV2::new(
            FOURS_CSID,
            path2.clone(),
            c4,
            vec![BlameParent::new(0, path1.clone(), c3, b3.clone())],
        )?;
        let b5 = BlameV2::new(
            FIVES_CSID,
            path2.clone(),
            c5,
            vec![BlameParent::new(0, path2.clone(), c4, b4.clone())],
        )?;

        assert_eq!(
            b1,
            BlameV2::Blame(BlameData {
                ranges: vec![BlameRangeIndexes {
                    offset: 0,
                    length: 4,
                    csid_index: 0,
                    path_index: 0,
                    origin_offset: 0,
                    parent: None,
                }],
                csids: vec_map! {
                    0 => ONES_CSID,
                },
                max_csid_index: 0,
                paths: vec![path1.clone()],
            }),
        );

        assert_eq!(
            b2,
            BlameV2::Blame(BlameData {
                ranges: vec![
                    BlameRangeIndexes {
                        offset: 0,
                        length: 1,
                        csid_index: 0,
                        path_index: 0,
                        origin_offset: 0,
                        parent: None,
                    },
                    BlameRangeIndexes {
                        offset: 1,
                        length: 2,
                        csid_index: 1,
                        path_index: 0,
                        origin_offset: 1,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 1,
                            length: 2,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 3,
                        length: 1,
                        csid_index: 0,
                        path_index: 0,
                        origin_offset: 3,
                        parent: None,
                    },
                ],
                csids: vec_map! {
                    0 => ONES_CSID,
                    1 => TWOS_CSID,
                },
                max_csid_index: 1,
                paths: vec![path1.clone()],
            }),
        );

        assert_eq!(
            b3,
            BlameV2::Blame(BlameData {
                ranges: vec![
                    BlameRangeIndexes {
                        offset: 0,
                        length: 1,
                        csid_index: 2,
                        path_index: 0,
                        origin_offset: 0,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 0,
                            length: 0,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 1,
                        length: 1,
                        csid_index: 0,
                        path_index: 0,
                        origin_offset: 0,
                        parent: None,
                    },
                    BlameRangeIndexes {
                        offset: 2,
                        length: 1,
                        csid_index: 1,
                        path_index: 0,
                        origin_offset: 2,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 1,
                            length: 2,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 3,
                        length: 1,
                        csid_index: 2,
                        path_index: 0,
                        origin_offset: 3,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 3,
                            length: 0,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 4,
                        length: 1,
                        csid_index: 0,
                        path_index: 0,
                        origin_offset: 3,
                        parent: None,
                    },
                ],
                csids: vec_map! {
                    0 => ONES_CSID,
                    1 => TWOS_CSID,
                    2 => THREES_CSID,
                },
                max_csid_index: 2,
                paths: vec![path1.clone()],
            }),
        );

        assert_eq!(
            b4,
            BlameV2::Blame(BlameData {
                ranges: vec![
                    BlameRangeIndexes {
                        offset: 0,
                        length: 1,
                        csid_index: 2,
                        path_index: 0,
                        origin_offset: 0,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 0,
                            length: 0,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 1,
                        length: 1,
                        csid_index: 0,
                        path_index: 0,
                        origin_offset: 0,
                        parent: None,
                    },
                    BlameRangeIndexes {
                        offset: 2,
                        length: 2,
                        csid_index: 3,
                        path_index: 1,
                        origin_offset: 2,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 2,
                            length: 1,
                            renamed_from_path_index: Some(0),
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 4,
                        length: 1,
                        csid_index: 2,
                        path_index: 0,
                        origin_offset: 3,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 3,
                            length: 0,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 5,
                        length: 1,
                        csid_index: 0,
                        path_index: 0,
                        origin_offset: 3,
                        parent: None,
                    },
                ],
                csids: vec_map! {
                    0 => ONES_CSID,
                    2 => THREES_CSID,
                    3 => FOURS_CSID,
                },
                max_csid_index: 3,
                paths: vec![path1.clone(), path2.clone()],
            }),
        );

        assert_eq!(
            b5,
            BlameV2::Blame(BlameData {
                ranges: vec![BlameRangeIndexes {
                    offset: 0,
                    length: 1,
                    csid_index: 0,
                    path_index: 0,
                    origin_offset: 0,
                    parent: None,
                },],
                csids: vec_map! {
                    0 => ONES_CSID,
                },
                max_csid_index: 4,
                paths: vec![path1.clone(), path2.clone()],
            }),
        );
        Ok(())
    }

    #[test]
    fn test_simple_merge() -> Result<()> {
        //  4
        //  |\
        //  | 3
        //  |
        //  2
        //  |
        //  1
        let path = MPath::new("path")?;

        let c1 = "one\ntwo\nthree\n";
        let c2 = "one\nfour\nfive\nthree\nsix\n";
        let c3 = "seven\neight\nnine\n";
        let c4 = "one\nfour\nfive\nseven\neight\nsix\nnine\n";

        let b1 = BlameV2::new(ONES_CSID, path.clone(), c1, vec![])?;
        let b2 = BlameV2::new(
            TWOS_CSID,
            path.clone(),
            c2,
            vec![BlameParent::new(0, path.clone(), c1, b1.clone())],
        )?;
        let b3 = BlameV2::new(THREES_CSID, path.clone(), c3, vec![])?;
        let b4 = BlameV2::new(
            FOURS_CSID,
            path.clone(),
            c4,
            vec![
                BlameParent::new(0, path.clone(), c2, b2.clone()),
                BlameParent::new(1, path.clone(), c3, b3.clone()),
            ],
        )?;

        assert_eq!(
            b4,
            BlameV2::Blame(BlameData {
                ranges: vec![
                    BlameRangeIndexes {
                        offset: 0,
                        length: 1,
                        csid_index: 0,
                        path_index: 0,
                        origin_offset: 0,
                        parent: None,
                    },
                    BlameRangeIndexes {
                        offset: 1,
                        length: 2,
                        csid_index: 1,
                        path_index: 0,
                        origin_offset: 1,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 1,
                            length: 1,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 3,
                        length: 2,
                        csid_index: 2,
                        path_index: 0,
                        origin_offset: 0,
                        parent: None,
                    },
                    BlameRangeIndexes {
                        offset: 5,
                        length: 1,
                        csid_index: 1,
                        path_index: 0,
                        origin_offset: 4,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 3,
                            length: 0,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 6,
                        length: 1,
                        csid_index: 2,
                        path_index: 0,
                        origin_offset: 2,
                        parent: None,
                    },
                ],
                csids: vec_map! {
                    0 => ONES_CSID,
                    1 => TWOS_CSID,
                    2 => THREES_CSID,
                },
                max_csid_index: 3,
                paths: vec![path.clone()],
            })
        );

        Ok(())
    }

    #[test]
    fn test_branch_and_merge() -> Result<()> {
        //  4
        //  |\
        //  | 3
        //  | |
        //  2 |
        //  |/
        //  1
        let path = MPath::new("path")?;

        let c1 = "one\ntwo\nthree\n";
        let c2 = "one\nfour\nfive\nthree\nsix\n";
        let c3 = "zero\none\nseven\neight\nnine\n";
        let c4 = "one\nfour\nten\nfive\nseven\neight\nsix\nnine\n";

        let b1 = BlameV2::new(ONES_CSID, path.clone(), c1, vec![])?;
        let b2 = BlameV2::new(
            TWOS_CSID,
            path.clone(),
            c2,
            vec![BlameParent::new(0, path.clone(), c1, b1.clone())],
        )?;
        let b3 = BlameV2::new(
            THREES_CSID,
            path.clone(),
            c3,
            vec![BlameParent::new(0, path.clone(), c1, b1.clone())],
        )?;
        let b4 = BlameV2::new(
            FOURS_CSID,
            path.clone(),
            c4,
            vec![
                BlameParent::new(0, path.clone(), c2, b2.clone()),
                BlameParent::new(0, path.clone(), c3, b3.clone()),
            ],
        )?;

        assert_eq!(
            b4,
            BlameV2::Blame(BlameData {
                ranges: vec![
                    BlameRangeIndexes {
                        offset: 0,
                        length: 1,
                        csid_index: 0,
                        path_index: 0,
                        origin_offset: 0,
                        parent: None,
                    },
                    BlameRangeIndexes {
                        offset: 1,
                        length: 1,
                        csid_index: 1,
                        path_index: 0,
                        origin_offset: 1,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 1,
                            length: 1,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 2,
                        length: 1,
                        csid_index: 3,
                        path_index: 0,
                        origin_offset: 2,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 2,
                            length: 0,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 3,
                        length: 1,
                        csid_index: 1,
                        path_index: 0,
                        origin_offset: 2,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 1,
                            length: 1,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 4,
                        length: 2,
                        csid_index: 2,
                        path_index: 0,
                        origin_offset: 2,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 1,
                            length: 2,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 6,
                        length: 1,
                        csid_index: 1,
                        path_index: 0,
                        origin_offset: 4,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 3,
                            length: 0,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 7,
                        length: 1,
                        csid_index: 2,
                        path_index: 0,
                        origin_offset: 4,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 1,
                            length: 2,
                            renamed_from_path_index: None,
                        }),
                    },
                ],
                csids: vec_map! {
                    0 => ONES_CSID,
                    1 => TWOS_CSID,
                    2 => THREES_CSID,
                    3 => FOURS_CSID,
                },
                max_csid_index: 3,
                paths: vec![path.clone()],
            })
        );

        Ok(())
    }

    #[test]
    fn test_origin_offset_merge() -> Result<()> {
        //  4
        //  |\
        //  | 3
        //  | |
        //  2 |
        //  |/
        //  1
        //
        // The merge commit deletes some middle part of a range from the
        // merged-in commit.  The range shouldn't be merged because of the
        // origin offset difference.
        //
        let path = MPath::new("path")?;

        let c1 = "one\ntwo\nthree\n";
        let c2 = "one\ntwo\nthree\nfour\n";
        let c3 = "zero\none\nseven\neight\nnine\nten\n";
        let c4 = "one\nseven\nnine\nten\nfour\n";

        let b1 = BlameV2::new(ONES_CSID, path.clone(), c1, vec![])?;
        let b2 = BlameV2::new(
            TWOS_CSID,
            path.clone(),
            c2,
            vec![BlameParent::new(0, path.clone(), c1, b1.clone())],
        )?;
        let b3 = BlameV2::new(
            THREES_CSID,
            path.clone(),
            c3,
            vec![BlameParent::new(0, path.clone(), c1, b1.clone())],
        )?;
        let b4 = BlameV2::new(
            FOURS_CSID,
            path.clone(),
            c4,
            vec![
                BlameParent::new(0, path.clone(), c2, b2.clone()),
                BlameParent::new(1, path.clone(), c3, b3.clone()),
            ],
        )?;

        assert_eq!(
            b4,
            BlameV2::Blame(BlameData {
                ranges: vec![
                    BlameRangeIndexes {
                        offset: 0,
                        length: 1,
                        csid_index: 0,
                        path_index: 0,
                        origin_offset: 0,
                        parent: None,
                    },
                    BlameRangeIndexes {
                        offset: 1,
                        length: 1,
                        csid_index: 2,
                        path_index: 0,
                        origin_offset: 2,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 1,
                            length: 2,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 2,
                        length: 2,
                        csid_index: 2,
                        path_index: 0,
                        origin_offset: 4,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 1,
                            length: 2,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 4,
                        length: 1,
                        csid_index: 1,
                        path_index: 0,
                        origin_offset: 3,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 3,
                            length: 0,
                            renamed_from_path_index: None,
                        }),
                    },
                ],
                csids: vec_map! {
                    0 => ONES_CSID,
                    1 => TWOS_CSID,
                    2 => THREES_CSID,
                },
                max_csid_index: 3,
                paths: vec![path.clone()],
            })
        );

        Ok(())
    }

    #[test]
    fn test_rejected_parents() -> Result<()> {
        //  4
        //  |\
        //  | 3(R)
        //  |
        //  2
        //  |
        //  1(R)
        let path = MPath::new("path")?;

        let c1 = "binary\0";
        let c2 = "one\ntwo\n";
        let c3 = "too big!";
        let c4 = "one\ntwo\nthree\nfour\n";

        let b1 = BlameV2::rejected(BlameRejected::Binary);
        let b2 = BlameV2::new(
            TWOS_CSID,
            path.clone(),
            c2,
            vec![BlameParent::new(0, path.clone(), c1, b1.clone())],
        )?;
        let b3 = BlameV2::rejected(BlameRejected::TooBig);
        let b4 = BlameV2::new(
            FOURS_CSID,
            path.clone(),
            c4,
            vec![
                BlameParent::new(0, path.clone(), c2, b2.clone()),
                BlameParent::new(1, path.clone(), c3, b3.clone()),
            ],
        )?;

        assert_eq!(
            b2,
            BlameV2::Blame(BlameData {
                ranges: vec![BlameRangeIndexes {
                    offset: 0,
                    length: 2,
                    csid_index: 0,
                    path_index: 0,
                    origin_offset: 0,
                    parent: None,
                }],
                csids: vec_map! {
                    0 => TWOS_CSID,
                },
                max_csid_index: 0,
                paths: vec![path.clone()],
            }),
        );

        assert_eq!(
            b4,
            BlameV2::Blame(BlameData {
                ranges: vec![
                    BlameRangeIndexes {
                        offset: 0,
                        length: 2,
                        csid_index: 0,
                        path_index: 0,
                        origin_offset: 0,
                        parent: None,
                    },
                    BlameRangeIndexes {
                        offset: 2,
                        length: 2,
                        csid_index: 1,
                        path_index: 0,
                        origin_offset: 2,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 2,
                            length: 0,
                            renamed_from_path_index: None,
                        }),
                    }
                ],
                csids: vec_map! {
                    0 => TWOS_CSID,
                    1 => FOURS_CSID,
                },
                max_csid_index: 1,
                paths: vec![path.clone()],
            }),
        );

        Ok(())
    }

    #[test]
    fn test_octopus_merge() -> Result<()> {
        //    4
        //   /|\
        //  1 2 3
        let path = MPath::new("path")?;

        let c1 = "one\ntwo\nthree\n";
        let c2 = "three\nfour\nfive\n";
        let c3 = "three\nsix\nnine\n";
        let c4 = "two\nthree\nfour\nfive\nsix\nseven\neight\nnine\n";

        let b1 = BlameV2::new(ONES_CSID, path.clone(), c1, vec![])?;
        let b2 = BlameV2::new(TWOS_CSID, path.clone(), c2, vec![])?;
        let b3 = BlameV2::new(THREES_CSID, path.clone(), c3, vec![])?;
        let b4 = BlameV2::new(
            FOURS_CSID,
            path.clone(),
            c4,
            vec![
                BlameParent::new(0, path.clone(), c1, b1.clone()),
                BlameParent::new(1, path.clone(), c2, b2.clone()),
                BlameParent::new(2, path.clone(), c3, b3.clone()),
            ],
        )?;

        assert_eq!(
            b4,
            BlameV2::Blame(BlameData {
                ranges: vec![
                    BlameRangeIndexes {
                        offset: 0,
                        length: 2,
                        csid_index: 0,
                        path_index: 0,
                        origin_offset: 1,
                        parent: None,
                    },
                    BlameRangeIndexes {
                        offset: 2,
                        length: 2,
                        csid_index: 1,
                        path_index: 0,
                        origin_offset: 1,
                        parent: None,
                    },
                    BlameRangeIndexes {
                        offset: 4,
                        length: 1,
                        csid_index: 2,
                        path_index: 0,
                        origin_offset: 1,
                        parent: None,
                    },
                    BlameRangeIndexes {
                        offset: 5,
                        length: 2,
                        csid_index: 3,
                        path_index: 0,
                        origin_offset: 5,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 3,
                            length: 0,
                            renamed_from_path_index: None,
                        }),
                    },
                    BlameRangeIndexes {
                        offset: 7,
                        length: 1,
                        csid_index: 2,
                        path_index: 0,
                        origin_offset: 2,
                        parent: None,
                    },
                ],
                csids: vec_map! {
                    0 => ONES_CSID,
                    1 => TWOS_CSID,
                    2 => THREES_CSID,
                    3 => FOURS_CSID,
                },
                max_csid_index: 3,
                paths: vec![path.clone()],
            }),
        );

        Ok(())
    }

    #[test]
    fn test_empty_file() -> Result<()> {
        let path = MPath::new("path")?;

        let c1 = "";
        let c2 = "data\n";
        let c3 = "";
        let c4 = "more data\n";

        let b1 = BlameV2::new(ONES_CSID, path.clone(), c1, vec![])?;
        let b2 = BlameV2::new(
            TWOS_CSID,
            path.clone(),
            c2,
            vec![BlameParent::new(0, path.clone(), c1, b1.clone())],
        )?;
        let b3 = BlameV2::new(
            THREES_CSID,
            path.clone(),
            c3,
            vec![BlameParent::new(0, path.clone(), c2, b2.clone())],
        )?;
        let b4 = BlameV2::new(
            FOURS_CSID,
            path.clone(),
            c4,
            vec![BlameParent::new(0, path.clone(), c3, b3.clone())],
        )?;

        assert_eq!(
            b1,
            BlameV2::Blame(BlameData {
                ranges: vec![],
                csids: vec_map! {},
                max_csid_index: 0,
                paths: vec![path.clone()],
            })
        );

        assert_eq!(
            b2,
            BlameV2::Blame(BlameData {
                ranges: vec![BlameRangeIndexes {
                    offset: 0,
                    length: 1,
                    csid_index: 1,
                    path_index: 0,
                    origin_offset: 0,
                    parent: Some(BlameParentIndexes {
                        parent_index: 0,
                        offset: 0,
                        length: 0,
                        renamed_from_path_index: None,
                    }),
                }],
                csids: vec_map! {1 => TWOS_CSID},
                max_csid_index: 1,
                paths: vec![path.clone()],
            })
        );

        assert_eq!(
            b3,
            BlameV2::Blame(BlameData {
                ranges: vec![],
                csids: vec_map! {},
                max_csid_index: 2,
                paths: vec![path.clone()],
            })
        );

        assert_eq!(
            b4,
            BlameV2::Blame(BlameData {
                ranges: vec![BlameRangeIndexes {
                    offset: 0,
                    length: 1,
                    csid_index: 3,
                    path_index: 0,
                    origin_offset: 0,
                    parent: Some(BlameParentIndexes {
                        parent_index: 0,
                        offset: 0,
                        length: 0,
                        renamed_from_path_index: None,
                    }),
                }],
                csids: vec_map! {3 => FOURS_CSID},
                max_csid_index: 3,
                paths: vec![path.clone()],
            })
        );
        Ok(())
    }

    #[test]
    fn test_skip_past_this_change() -> Result<()> {
        //  6
        //  |
        //  5
        //  |\
        //  | 4
        //  | |\
        //  | X 3
        //  |  /
        //  2 /
        //  |/
        //  1
        //
        //  Commit 4 is a merge where the file is not present in the first
        //  parent.  Commits 2 and 4 also rename the file to path2, and
        //  commit 5 renames it to path3.
        let path1 = MPath::new("path1")?;
        let path2 = MPath::new("path2")?;
        let path3 = MPath::new("path3")?;

        let c1 = "one\nthree\nfive\n";
        let c2 = "one\ntwo\nthree\nfive\n";
        let c3 = "zero\none\nthree\nfour\nfive\n";
        let c4 = "zero\none\nsix\nseven\n";
        let c5 = "zero\nhalf\ntwo\nsix\nseven\n";
        let c6 = "zero\nhalf\ntwo\nthree\nfour\nfive\nsix\nseven\n";

        let b1 = BlameV2::new(ONES_CSID, path1.clone(), c1, vec![])?;
        let b2 = BlameV2::new(
            TWOS_CSID,
            path2.clone(),
            c2,
            vec![BlameParent::new(0, path1.clone(), c1, b1.clone())],
        )?;
        let b3 = BlameV2::new(
            THREES_CSID,
            path1.clone(),
            c3,
            vec![BlameParent::new(0, path1.clone(), c1, b1.clone())],
        )?;
        let b4 = BlameV2::new(
            FOURS_CSID,
            path2.clone(),
            c4,
            vec![
                // BlameParent 0 is omitted as the file is not present there.
                BlameParent::new(1, path1.clone(), c3, b3.clone()),
            ],
        )?;
        let b5 = BlameV2::new(
            FIVES_CSID,
            path3.clone(),
            c5,
            vec![
                BlameParent::new(0, path2.clone(), c2, b2.clone()),
                BlameParent::new(1, path2.clone(), c4, b4.clone()),
            ],
        )?;
        let b6 = BlameV2::new(
            SIXES_CSID,
            path3.clone(),
            c6,
            vec![BlameParent::new(0, path3.clone(), c5, b5.clone())],
        )?;

        assert_eq!(
            b6,
            BlameV2::Blame(BlameData {
                ranges: vec![
                    BlameRangeIndexes {
                        offset: 0,
                        length: 1,
                        csid_index: 2,
                        path_index: 0,
                        origin_offset: 0,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 0,
                            length: 0,
                            renamed_from_path_index: None,
                        }),
                        // Skip past this change goes to p1 of commit 3
                        // (commit 1), inserting before line 0.
                    },
                    BlameRangeIndexes {
                        offset: 1,
                        length: 1,
                        csid_index: 4,
                        path_index: 2,
                        origin_offset: 1,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 0,
                            length: 1,
                            renamed_from_path_index: Some(1),
                        }),
                        // Skip past this change goes to p1 of commit 5
                        // (commit 2), replacing line 0 ("one" -> "zero, half").
                        // The file was renamed from path2.
                    },
                    BlameRangeIndexes {
                        offset: 2,
                        length: 1,
                        csid_index: 1,
                        path_index: 1,
                        origin_offset: 1,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 1,
                            length: 0,
                            renamed_from_path_index: Some(0),
                        }),
                        // Skip past this change goes to p1 of commit 2
                        // (commit 2), inserting before line 1.  The file
                        // was renamed from path1.
                    },
                    BlameRangeIndexes {
                        offset: 3,
                        length: 3,
                        csid_index: 5,
                        path_index: 2,
                        origin_offset: 3,
                        parent: Some(BlameParentIndexes {
                            parent_index: 0,
                            offset: 3,
                            length: 0,
                            renamed_from_path_index: None,
                        }),
                        // Skip past this change goes to p1 of commit 6
                        // (commit 5), inserting before line 3.
                    },
                    BlameRangeIndexes {
                        offset: 6,
                        length: 2,
                        csid_index: 3,
                        path_index: 1,
                        origin_offset: 2,
                        parent: Some(BlameParentIndexes {
                            parent_index: 1,
                            offset: 2,
                            length: 3,
                            renamed_from_path_index: Some(0),
                        }),
                        // Skip past this change goes to p2 of commit 4
                        // (commit 3), replacing lines 2 to 5.  The file was
                        // renamed from path1.
                    },
                ],
                csids: vec_map! {
                    1 => TWOS_CSID,
                    2 => THREES_CSID,
                    3 => FOURS_CSID,
                    4 => FIVES_CSID,
                    5 => SIXES_CSID,
                },
                max_csid_index: 5,
                paths: vec![path1.clone(), path2.clone(), path3.clone()],
            })
        );
        Ok(())
    }

    #[test]
    fn test_mutated_blame_simple() -> Result<()> {
        // 4
        // |
        // 3
        // |\
        // 2 1
        // Original blame is at 2, mutated to 1.
        let path = MPath::new("path")?;
        let c1 = "Mutant\nText\nHere\n";
        let c2 = "Plain\nText\n";
        let c3 = "Rich\nText\n";
        let c4 = "Rich\nTea\nText\n";

        let b1 = BlameV2::new(ONES_CSID, path.clone(), c1, vec![])?;
        let b2 = BlameV2::new(TWOS_CSID, path.clone(), c2, vec![])?;
        let b3_orig = BlameV2::new(
            THREES_CSID,
            path.clone(),
            c3,
            vec![BlameParent::new(0, path.clone(), c2, b2.clone())],
        )?;
        let b3_mutant = BlameV2::new(
            THREES_CSID,
            path.clone(),
            c3,
            vec![BlameParent::new(0, path.clone(), c1, b1.clone())],
        )?;
        let b4_orig = BlameV2::new(
            FOURS_CSID,
            path.clone(),
            c4,
            vec![BlameParent::new(0, path.clone(), c3, b3_orig.clone())],
        )?;
        let b4_mutant = BlameV2::new(
            FOURS_CSID,
            path.clone(),
            c4,
            vec![BlameParent::new(0, path.clone(), c3, b3_mutant.clone())],
        )?;

        let mut b4_fixed = b4_orig.clone();
        b4_fixed.apply_mutable_change(&b3_orig, &b3_mutant)?;

        assert_eq!(b4_fixed, b4_mutant);

        Ok(())
    }

    // No change test
    #[test]
    fn test_mutated_blame_mutant_ignored() -> Result<()> {
        // 4
        // |
        // 3
        // |\
        // 2 1
        // Original blame is at 2, mutated to 1.
        let path = MPath::new("path")?;
        let c1 = "Mutant\nText\nHere\n";
        let c2 = "Plain\nText\n";
        let c3 = "Completely\nNew\n";
        let c4 = "And\nDifferent\n";

        let b1 = BlameV2::new(ONES_CSID, path.clone(), c1, vec![])?;
        let b2 = BlameV2::new(TWOS_CSID, path.clone(), c2, vec![])?;
        let b3_orig = BlameV2::new(
            THREES_CSID,
            path.clone(),
            c3,
            vec![BlameParent::new(0, path.clone(), c2, b2.clone())],
        )?;
        let b3_mutant = BlameV2::new(
            THREES_CSID,
            path.clone(),
            c3,
            vec![BlameParent::new(0, path.clone(), c1, b1.clone())],
        )?;
        let b4_orig = BlameV2::new(
            FOURS_CSID,
            path.clone(),
            c4,
            vec![BlameParent::new(0, path.clone(), c3, b3_orig.clone())],
        )?;
        let b4_mutant = BlameV2::new(
            FOURS_CSID,
            path.clone(),
            c4,
            vec![BlameParent::new(0, path.clone(), c3, b3_mutant.clone())],
        )?;

        let mut b4_fixed = b4_orig.clone();
        b4_fixed.apply_mutable_change(&b3_orig, &b3_mutant)?;

        // Blame for 3 differs, because it changes everything and has very different
        // parents
        assert_ne!(b3_orig, b3_mutant);
        // But 4 is also a rewrite of everything, and is trivial as a result - it
        // goes no further back than 3
        assert_eq!(b4_fixed, b4_mutant);
        assert_eq!(b4_orig, b4_mutant);

        Ok(())
    }

    #[test]
    fn test_mutated_blame_rename() -> Result<()> {
        // 4
        // |
        // 3
        // |\
        // 2 1
        // Original blame is at 2, mutated to 1.
        // Mutable blame changed the name going 3 to 1
        let path1 = MPath::new("path1")?;
        let path2 = MPath::new("path2")?;
        let c1 = "Mutant\nText\nHere\n";
        let c2 = "Plain\nText\n";
        let c3 = "Rich\nText\n";
        let c4 = "Rich\nTea\nText\n";

        let b1 = BlameV2::new(ONES_CSID, path2.clone(), c1, vec![])?;
        let b2 = BlameV2::new(TWOS_CSID, path1.clone(), c2, vec![])?;
        let b3_orig = BlameV2::new(
            THREES_CSID,
            path1.clone(),
            c3,
            vec![BlameParent::new(0, path1.clone(), c2, b2.clone())],
        )?;
        let b3_mutant = BlameV2::new(
            THREES_CSID,
            path1.clone(),
            c3,
            vec![BlameParent::new(0, path2.clone(), c1, b1.clone())],
        )?;
        let b4_orig = BlameV2::new(
            FOURS_CSID,
            path1.clone(),
            c4,
            vec![BlameParent::new(0, path1.clone(), c3, b3_orig.clone())],
        )?;
        let b4_mutant = BlameV2::new(
            FOURS_CSID,
            path1.clone(),
            c4,
            vec![BlameParent::new(0, path1.clone(), c3, b3_mutant.clone())],
        )?;

        let mut b4_fixed = b4_orig.clone();
        b4_fixed.apply_mutable_change(&b3_orig, &b3_mutant)?;

        assert_eq!(b4_fixed, b4_mutant);

        Ok(())
    }

    #[test]
    fn test_mutated_blame_long_path() -> Result<()> {
        // 6
        // |
        // 5
        // |
        // 4
        // |
        // 3
        // |\
        // 2 1
        // Original blame is at 2, mutated to 1.
        let path = MPath::new("path")?;
        let c1 = "Mutant\nText\nHere\n";
        let c2 = "Plain\nText\n";
        let c3 = "Rich\nText\n";
        let c4 = "Rich\nTea\nText\n";
        let c5 = "Rich\nTea\nText\nHere\n";
        let c6 = "Digestive and\nRich\nTea\nHere\n";

        let b1 = BlameV2::new(ONES_CSID, path.clone(), c1, vec![])?;
        let b2 = BlameV2::new(TWOS_CSID, path.clone(), c2, vec![])?;
        let b3_orig = BlameV2::new(
            THREES_CSID,
            path.clone(),
            c3,
            vec![BlameParent::new(0, path.clone(), c2, b2.clone())],
        )?;
        let b4 = BlameV2::new(
            FOURS_CSID,
            path.clone(),
            c4,
            vec![BlameParent::new(0, path.clone(), c3, b3_orig.clone())],
        )?;
        let b5 = BlameV2::new(
            FIVES_CSID,
            path.clone(),
            c5,
            vec![BlameParent::new(0, path.clone(), c4, b4.clone())],
        )?;
        let b6_orig = BlameV2::new(
            SIXES_CSID,
            path.clone(),
            c6,
            vec![BlameParent::new(0, path.clone(), c5, b5.clone())],
        )?;

        let b3_mutant = BlameV2::new(
            THREES_CSID,
            path.clone(),
            c3,
            vec![BlameParent::new(0, path.clone(), c1, b1.clone())],
        )?;
        let b4_mutant = BlameV2::new(
            FOURS_CSID,
            path.clone(),
            c4,
            vec![BlameParent::new(0, path.clone(), c3, b3_mutant.clone())],
        )?;
        let b5_mutant = BlameV2::new(
            FIVES_CSID,
            path.clone(),
            c5,
            vec![BlameParent::new(0, path.clone(), c4, b4_mutant.clone())],
        )?;
        let b6_mutant = BlameV2::new(
            SIXES_CSID,
            path.clone(),
            c6,
            vec![BlameParent::new(0, path.clone(), c5, b5_mutant.clone())],
        )?;

        let mut b6_fixed = b6_orig.clone();
        b6_fixed.apply_mutable_change(&b3_orig, &b3_mutant)?;

        assert_eq!(b6_fixed, b6_mutant);

        Ok(())
    }

    #[test]
    fn test_mutated_blame_shared_ancestor() -> Result<()> {
        // 7
        // |
        // 6
        // |\
        // 4 5
        // |\|\
        // 1 2 3
        // Original blame is at 4, mutated to 5.
        // Tricksyness is because 4 is a merge of 1 and 2, while 5 is a merge of 2 and 3
        let path = MPath::new("path")?;
        let c1 = "Plain\n";
        let c2 = "Text\n";
        let c3 = "Rich\n";
        let c4 = "Plain\nText\n";
        let c5 = "Rich\nText\n";
        let c6 = "Rich\nor\nPlain\nText\n";
        let c7 = "Rich\nor\nPlain\nText\n";

        let b1 = BlameV2::new(ONES_CSID, path.clone(), c1, vec![])?;
        let b2 = BlameV2::new(TWOS_CSID, path.clone(), c2, vec![])?;
        let b3 = BlameV2::new(THREES_CSID, path.clone(), c3, vec![])?;
        let b4 = BlameV2::new(
            FOURS_CSID,
            path.clone(),
            c4,
            vec![
                BlameParent::new(0, path.clone(), c1, b1.clone()),
                BlameParent::new(1, path.clone(), c2, b2.clone()),
            ],
        )?;
        let b5 = BlameV2::new(
            FIVES_CSID,
            path.clone(),
            c5,
            vec![
                BlameParent::new(0, path.clone(), c2, b2.clone()),
                BlameParent::new(1, path.clone(), c3, b3.clone()),
            ],
        )?;
        let b6_orig = BlameV2::new(
            SIXES_CSID,
            path.clone(),
            c6,
            vec![BlameParent::new(0, path.clone(), c4, b4.clone())],
        )?;
        let b6_mutant = BlameV2::new(
            SIXES_CSID,
            path.clone(),
            c6,
            vec![BlameParent::new(0, path.clone(), c5, b5.clone())],
        )?;
        let b7_orig = BlameV2::new(
            SEVENS_CSID,
            path.clone(),
            c7,
            vec![BlameParent::new(0, path.clone(), c6, b6_orig.clone())],
        )?;
        let b7_mutant = BlameV2::new(
            SEVENS_CSID,
            path.clone(),
            c7,
            vec![BlameParent::new(0, path.clone(), c6, b6_mutant.clone())],
        )?;

        let mut b7_fixed = b7_orig.clone();
        b7_fixed.apply_mutable_change(&b6_orig, &b6_mutant)?;

        // The mutant blame is slightly different, because it omits a changeset hole
        // you would have if you followed the mutant blame down its hole.
        // So just do a semantic check
        assert!(b7_fixed.ranges()?.eq(b7_mutant.ranges()?));

        Ok(())
    }
}
