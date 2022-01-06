/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::changeset_path::ChangesetPathContentContext;

/// A path difference between two commits.
///
/// A ChangesetPathDiffContext shows the difference between two corresponding
/// files in the commits.
///
/// The changed, copied and moved variants contain the items in the same
/// order as the commits that were compared, i.e. in `a.diff(b)`, they
/// will contain `(a, b)`.  This usually means the destination is first.
#[derive(Clone, Debug)]
pub enum ChangesetPathDiffContext {
    Added(ChangesetPathContentContext),
    Removed(ChangesetPathContentContext),
    Changed(ChangesetPathContentContext, ChangesetPathContentContext),
    Copied(ChangesetPathContentContext, ChangesetPathContentContext),
    Moved(ChangesetPathContentContext, ChangesetPathContentContext),
}
