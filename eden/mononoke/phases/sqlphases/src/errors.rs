/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

#[derive(Debug, Eq, Error, PartialEq)]
pub enum SqlPhasesError {
    #[error("invalid phase value: {0}")]
    ValueError(String),
    #[error("failed to parse phase value from {0} bytes")]
    ParseError(usize),
}
