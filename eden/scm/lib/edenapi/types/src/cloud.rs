/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(any(test, feature = "for-tests"))]
use quickcheck_arbitrary_derive::Arbitrary;
use serde_derive::Deserialize;
use serde_derive::Serialize;
use type_macros::auto_wire;

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct WorkspaceData {
    #[id(0)]
    pub name: String,
    #[id(1)]
    pub reponame: String,
    #[id(2)]
    pub version: u64,
    #[id(3)]
    pub archived: bool,
    #[id(4)]
    pub timestamp: i64,
}

// Types for cloud/workspace

#[auto_wire]
#[derive(Clone, Default, Debug, Deserialize, Serialize, Eq, PartialEq)]
#[cfg_attr(any(test, feature = "for-tests"), derive(Arbitrary))]
pub struct CloudWorkspaceRequest {
    #[id(0)]
    pub workspace: String,
    #[id(1)]
    pub reponame: String,
}
