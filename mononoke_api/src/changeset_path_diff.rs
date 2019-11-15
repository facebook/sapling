/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::changeset_path::ChangesetPathContext;

/// A path difference between two commits.
///
/// A ChangesetPathDiffContext shows the difference between two corresponding
/// files in the commits.
#[derive(Clone)]
pub enum ChangesetPathDiffContext {
    Added(ChangesetPathContext),
    Removed(ChangesetPathContext),
    Changed(ChangesetPathContext, ChangesetPathContext),
    Copied(ChangesetPathContext, ChangesetPathContext),
    Moved(ChangesetPathContext, ChangesetPathContext),
}
