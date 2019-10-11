/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

pub use failure_ext::{Error, Fail, Result};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "invalid manifest description: {}", _0)]
    InvalidManifestDescription(String),
    #[fail(display = "invalid path map: {}", _0)]
    InvalidPathMap(String),
    #[fail(display = "invalid directory hash map: {}", _0)]
    InvalidDirectoryHashes(String),
}
