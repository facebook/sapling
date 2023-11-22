/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use types::HgId;
use types::RepoPathBuf;

pub struct MergeState {
    // commits being merged
    local: Option<HgId>,
    other: Option<HgId>,

    // contextual labels for local/other/base
    labels: Vec<String>,

    // conflicting files
    files: HashMap<RepoPathBuf, FileInfo>,

    // merge driver definition at start of merge so we can detect merge driver
    // config changing during merge.
    merge_driver: Option<(String, MergeDriverState)>,
}

pub struct FileInfo {
    // arbitrary key->value data (seems to only be used for "ancestorlinknode")
    extras: HashMap<String, String>,
    state: ConflictState,

    // An opaque-to-Rust tuple of data.
    //
    // For path conflicts it contains:
    //
    //    [
    //      <renamed name>,
    //      l(ocal) | r(emote),
    //    ]
    // For other conflicts it contains:
    //
    //    [
    //      <hash of "local" file path>,
    //      <local file path>,
    //      <ancestor file path>,
    //      <ancestor file node hex>,
    //      <other file path>,
    //      <other file node hex>,
    //      <local file flags>,
    //    ]
    data: Vec<String>,
}

pub enum ConflictState {
    Unresolved,
    Resolved,
    UnresolvedPath,
    ResolvedPath,
    DriverResolved,
}

pub enum MergeDriverState {
    Unmarked,
    Marked,
    Success,
}
