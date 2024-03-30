/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mononoke_types::DateTime as MononokeDateTime;

use crate::WorkspaceContents;
#[allow(unused)]
pub(crate) struct WorkspaceVersion {
    workspace: WorkspaceContents,
    version: u64,
    date: MononokeDateTime,
    timestamp: i64,
    archived: bool,
}
