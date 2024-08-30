/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub struct WorkspaceData {
    pub name: String,
    pub reponame: String,
    pub version: u64,
    pub archived: bool,
    pub timestamp: i64,
}
