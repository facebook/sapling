// Copyright Facebook, Inc. 2019.

//! edenapi-types: common types shared between Mercurial and Mononoke
//!
//! This crate contains types that are shared between the Mercurial and
//! Mononoke codebases for the purpose of data interchange. It is
//! necessary to put these types here because Mercurial cannot depend
//! on anything outside of the fbcode/scm/hg directory, and thus cannot
//! directly link to Mononoke code. Similarly, we cannot use Thrift data
//! types since that would require using Buck for builds (whereas Mercurial
//! must also support Cargo builds).

use serde_derive::{Deserialize, Serialize};

use types::{Key, NodeInfo};

#[derive(Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub key: Key,
    pub nodeinfo: NodeInfo,
}
