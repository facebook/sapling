/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use serde::Deserialize;
use serde::Serialize;
use type_macros::auto_wire;
use types::RepoPathBuf;

use crate::CommitId;

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct SuffixQueryRequest {
    #[id(0)]
    pub commit: CommitId,
    #[id(1)]
    pub basename_suffixes: Vec<String>,
}

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct SuffixQueryResponse {
    #[id(0)]
    pub file_path: RepoPathBuf,
}
