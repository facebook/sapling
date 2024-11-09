/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use serde::Deserialize;
use serde::Serialize;
use type_macros::auto_wire;
use types::HgId;
use types::Key;
use types::RepoPathBuf;

use crate::ServerError;

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct BlameRequest {
    // The commit+path pairs we are blaming.
    #[id(0)]
    pub files: Vec<Key>,
}

#[auto_wire]
#[derive(Clone, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct BlameResult {
    #[id(0)]
    pub file: Key,
    #[id(1)]
    #[no_default]
    pub data: Result<BlameData, ServerError>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct BlameData {
    #[id(0)]
    pub line_ranges: Vec<BlameLineRange>,
    #[id(1)]
    pub commits: Vec<HgId>,
    #[id(2)]
    pub paths: Vec<RepoPathBuf>,
}

// BlameLineRange represents a sequence of consecutive lines that originated
// from the same commit.
#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct BlameLineRange {
    // First line of this range (zero indexed).
    #[id(0)]
    pub line_offset: u32,
    // Number of lines in this range.
    #[id(1)]
    pub line_count: u32,
    // What commit added this line (index into BlameData.commits)
    #[id(2)]
    pub commit_index: u32,
    // Path this line was originally added to  (index into BlameData.paths)
    #[id(3)]
    pub path_index: u32,
    // First line of this range in origin commit (zero indexed).
    #[id(4)]
    pub origin_line_offset: u32,
}
