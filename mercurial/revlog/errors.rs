/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

pub use failure_ext::{bail, ensure_msg, format_err, prelude::*, Error, Result, ResultExt};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Bundle2Decode: {0}")]
    Bundle2Decode(String),
    #[error("Revlog: {0}")]
    Revlog(String),
    #[error("Repo: {0}")]
    Repo(String),
    #[error("Path: {0}")]
    Path(String),
    #[error("Unknown requirement: {0}")]
    UnknownReq(String),
    #[error("invalid Thrift structure '{0}': {1}")]
    InvalidThrift(String, String),
}
