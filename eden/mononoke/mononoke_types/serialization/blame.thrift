/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! ------------
//! IMPORTANT!!!
//! ------------
//! Do not change the order of the fields! Changing the order of the fields
//! results in compatible but *not* identical serializations, so hashes will
//! change.
//! ------------
//! IMPORTANT!!!
//! ------------

include "eden/mononoke/mononoke_types/serialization/id.thrift"
include "eden/mononoke/mononoke_types/serialization/path.thrift"

typedef i32 BlameChangeset (rust.newtype)
typedef i32 BlamePath (rust.newtype)

enum BlameRejected {
  TooBig = 0,
  Binary = 1,
}

struct BlameRangeV2 {
  // Length (in lines) of this range.  The offset of a range is implicit from
  // the sum of the lengths of the prior ranges.
  1: i32 length;

  // Index into csids of the changeset that introduced these lines.
  2: BlameChangeset csid_index;

  // Index into paths of the path of this file when this line was introduced.
  3: BlamePath path_index;

  // The offset of this range at the time that this line was introduced.
  4: i32 origin_offset;

  // "Skip past this change" support.
  //
  // The following fields allows clients to provide an accurate "skip past this
  // change" feature.  From any given range in a blame file, "skip path this
  // change" will direct the user to a *range* of lines in the same file in
  // one of the parents of the changeset that this range is blamed to (i.e.,
  // the changeset specified by `csid_index`).
  //
  // If the range originates in the first version of the file (i.e. this is
  // a root commit for the file), then these fields will not be present.
  //
  // In the simplest case, the target file is the file with the name specified
  // by `path_index` in the first parent of the changeset specified by
  // `csid_index`.
  //
  // The range of lines specified by `parent_offset` and `parent_length`
  // corresponds to the lines that were *replaced* by this range.  In the case
  // of pure insertions, `parent_length` will be 0, indicating that the new
  // lines were *inserted* before the line at `parent_offset`.
  //
  // If the file was renamed during this change, then `renamed_from_path_index`
  // will contain the index into paths of the name of the file before the rename.
  // This should be used in preference to `path_index` to find the target file.
  //
  // If the target commit was a merge commit, and the file was not present in
  // the first parent, then `parent_index` will contain the index (in the list
  // of parents in the bonsai changeset) of the first parent that does contain
  // the file.
  //
  // Thus, the algorithm for finding the destination for "skip past this change"
  // is:
  //
  //  1. Look up `csid_index` in `csids` to find the blamed changeset, and load
  //     its BonsaiChangeset.
  //  2. Find the parent at `parent_index` in the list of parents, or the first
  //     parent if `parent_index` is not present.  This is the target changeset.
  //  3. Look up the path at `renamed_from_path_index` in `paths`, or
  //     `path_index` if `renamed_from_path_index` is not present.  This is the
  //     target path.
  //  4. Load the file at the target path in the target changeset.
  //  5. Jump to the range of lines of length `parent_length` starting at
  //     `parent_offset`.  This is the range of lines that were changed by the
  //     change we are skipping over.  Note that the length might be 0,
  //     indicating an insertion.

  // The offset of this range in the file before this range was introduced that
  // was replaced by this range.  Not present for root commits.
  5: optional i32 parent_offset;

  // The length of the range in the file before this range was introduced that
  // was replaced by this range.  Not present for root commits.
  6: optional i32 parent_length;

  // If this file was being renamed when this line was introduced, this is
  // the index into paths of the original path.  Not present for root commits
  // or if the file has the same name as path_index.
  7: optional BlamePath renamed_from_path_index;

  // If this is a merge commit, and the file is not in the first parent, then
  // this is the index of the first parent that contains the file that contains
  // the range that this range replaces.
  //
  // Not present for ranges in root commits or commits with single parents, or
  // if the file is present in the first parent.
  //
  // Note that this is an index into the list of parents in the bonsai
  // changeset, and *not* an index into csids.
  8: optional i32 parent_index;
} (rust.exhaustive)

struct BlameDataV2 {
  // A list of ranges that describe when the lines of this file were
  // introduced.
  1: list<BlameRangeV2> ranges;

  // A mapping of integer indexes to changeset IDs that is used to reduce the
  // repetition of data in ranges.
  //
  // Changeset ID indexes are stable for p1 parents, i.e. a changeset ID's
  // index will not change over the history of a file unless the file is merged
  // in a changeset, in which case only the indexes in the first parent of the
  // changeset are preserved.

  // Changesets are removed from this map when all lines that were added in the
  // changeset are moved and none of the ranges reference it.  This means there
  // are gaps in this mapping, and so a map is used.
  2: map<i32, id.ChangesetId> (
    rust.type = "sorted_vector_map::SortedVectorMap",
  ) csids;

  // The maximum index that is assigned to a changeset id.  This is also the
  // index that would be assigned to the current changeset, as long as the
  // changeset adds new lines.  If the changeset only deletes or merges lines,
  // then this index will not appear in the csids map.
  3: BlameChangeset max_csid_index;

  // The list of paths that this file has been located at.  This is used to
  // reduce repetition of data in ranges.  Since files are not often moved, and
  // for simplicity, this includes all paths the file has ever been located at,
  // even if they are no longer referenced by any of the ranges.
  4: list<path.NonRootMPath> paths;
} (rust.exhaustive)

union BlameV2 {
  // This version of the file contains full blame information.
  1: BlameDataV2 full_blame;

  // This version of the file was rejected for blaming.
  2: BlameRejected rejected;
}
