/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Error;
use cxx::UniquePtr;

use crate::ffi::ffi;

/// Translate anyhow errors from the backinstore
/// to SaplingBackingStoreError in C++ for EdenFS to consume
pub(crate) fn into_backingstore_err(err: Error) -> UniquePtr<ffi::SaplingBackingStoreError> {
    ffi::backingstore_error(&format!("{:?}", err), ffi::BackingStoreErrorKind::Generic)
}
