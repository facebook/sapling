/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use failure::Fail;
pub use failure_ext::{Error, Result};

use mononoke_types::ChangesetId;

#[derive(Debug, Eq, Fail, PartialEq)]
pub enum ErrorKind {
    #[fail(
        display = "Duplicate changeset {} has different parents: {:?} vs {:?}",
        _0, _1, _2
    )]
    DuplicateInsertionInconsistency(ChangesetId, Vec<ChangesetId>, Vec<ChangesetId>),
    #[fail(display = "Missing parents")]
    MissingParents(Vec<ChangesetId>),
}
