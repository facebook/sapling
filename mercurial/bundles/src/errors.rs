/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::part_header::{PartHeader, PartHeaderType};

pub use failure_ext::{Error, Result, ResultExt};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("bundle2 decode error: {0}")]
    Bundle2Decode(String),
    #[error("changegroup decode error: {0}")]
    CgDecode(String),
    #[error("changegroup2 encode error: {0}")]
    Cg2Encode(String),
    #[error("wirepack decode error: {0}")]
    WirePackDecode(String),
    #[error("wirepack encode error: {0}")]
    WirePackEncode(String),
    #[error("bundle2 encode error: {0}")]
    Bundle2Encode(String),
    #[error("bundle2 chunk error: {0}")]
    Bundle2Chunk(String),
    #[error("invalid delta: {0}")]
    InvalidDelta(String),
    #[error("invalid wire pack entry: {0}")]
    InvalidWirePackEntry(String),
    #[error("unknown part type: {0:?}")]
    BundleUnknownPart(PartHeader),
    #[error("unknown params for bundle2 part '{0:?}': {1:?}")]
    BundleUnknownPartParams(PartHeaderType, Vec<String>),
    #[error("error while generating listkey part")]
    ListkeyGeneration,
    #[error("error while generating phase-heads part")]
    PhaseHeadsGeneration,
}

impl ErrorKind {
    pub fn is_app_error(&self) -> bool {
        match self {
            &ErrorKind::BundleUnknownPart(_) | &ErrorKind::BundleUnknownPartParams(..) => true,
            _ => false,
        }
    }
}
