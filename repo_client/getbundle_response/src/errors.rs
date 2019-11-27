/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

pub use failure_ext::{Error, Result, ResultExt};
use thiserror::Error;

use mercurial_types::HgChangesetId;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Bonsai not found for hg changeset: {0:?}")]
    BonsaiNotFoundForHgChangeset(HgChangesetId),
}
