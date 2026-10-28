/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! The parts of the bridge that need internal crates. oss.rs provides the
//! same interface for open-source builds.

use cxx::UniquePtr;
pub use iobuf::IOBuf;

pub fn blob_into_iobuf(blob: blob::Blob) -> UniquePtr<IOBuf> {
    blob.into_iobuf().into()
}
