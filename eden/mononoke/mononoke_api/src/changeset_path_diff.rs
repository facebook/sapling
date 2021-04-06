/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::changeset_path::ChangesetPathContentContext;

/// A path difference between two commits.
///
/// A ChangesetPathDiffContext shows the difference between two corresponding
/// files in the commits.
#[derive(Clone, Debug)]
pub enum ChangesetPathDiffContext {
    Added(ChangesetPathContentContext),
    Removed(ChangesetPathContentContext),
    Changed(ChangesetPathContentContext, ChangesetPathContentContext),
    Copied(ChangesetPathContentContext, ChangesetPathContentContext),
    Moved(ChangesetPathContentContext, ChangesetPathContentContext),
}
