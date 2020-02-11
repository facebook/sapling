/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

#[derive(Debug, Eq, Error, PartialEq)]
pub enum ErrorKind {
    #[error("failed to get/set phase, reason: {0}")]
    PhasesError(String),
}
