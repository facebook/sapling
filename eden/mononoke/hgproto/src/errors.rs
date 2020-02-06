/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Unimplemented operation '{0}'")]
    Unimplemented(String),
    #[error("command parse failed for '{0}'")]
    CommandParse(String),
    #[error("unconsumed data left after parsing '{0}'")]
    UnconsumedData(String),
    #[error("malformed batch with command '{0}'")]
    BatchInvalid(String),
    #[error("malformed bundle2 '{0}'")]
    Bundle2Invalid(String),
    #[error("unknown escape character in batch command '{0}'")]
    BatchEscape(u8),
    #[error("Repo error")]
    RepoError,
    #[error("cannot serve revlog repos")]
    CantServeRevlogRepo,
}
