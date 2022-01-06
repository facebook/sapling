/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

#[derive(Debug, Eq, Error, PartialEq)]
pub enum PhasesError {
    #[error("invalid phase enumeration value: {0}")]
    EnumError(u32),
}
