/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use mercurial_types::HgChangesetId;
/// User-defined bookmarks
#[allow(unused)]
pub(crate) struct WorkspaceLocalBookmark {
    name: String,
    node: HgChangesetId,
}
