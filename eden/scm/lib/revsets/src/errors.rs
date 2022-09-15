/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

#[derive(Error, Debug)]
pub enum RevsetLookupError {
    #[error("ambiguous identifier for '{0}': {1} available")]
    AmbiguousIdentifier(String, String),

    #[error(transparent)]
    DagError(#[from] dag::Error),

    #[error("unknown revision '{0}'")]
    RevsetNotFound(String),
}
