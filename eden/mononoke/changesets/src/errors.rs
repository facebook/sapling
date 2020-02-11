/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

use mononoke_types::ChangesetId;

#[derive(Debug, Eq, Error, PartialEq)]
pub enum ErrorKind {
    #[error("Duplicate changeset {0} has different parents: {1:?} vs {2:?}")]
    DuplicateInsertionInconsistency(ChangesetId, Vec<ChangesetId>, Vec<ChangesetId>),
    #[error("Missing parents")]
    MissingParents(Vec<ChangesetId>),
}
