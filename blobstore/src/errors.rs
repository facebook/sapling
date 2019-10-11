/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use failure_ext::Fail;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Blob {} not found in blobstore", _0)]
    NotFound(String),
    #[fail(display = "Error while opening state for blob store")]
    StateOpen,
}
