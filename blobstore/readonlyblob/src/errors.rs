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
    #[fail(display = "Attempt to put to ReadOnlyBlobstore for key {}", _0)]
    ReadOnlyPut(String),
}
