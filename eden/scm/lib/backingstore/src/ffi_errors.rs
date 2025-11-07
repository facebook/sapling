/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Error;
use cxx::UniquePtr;
use edenapi::SaplingRemoteApiError;
use revisionstore::error::LfsFetchError;

use crate::ffi::ffi::BackingStoreErrorKind;
use crate::ffi::ffi::SaplingBackingStoreError;
use crate::ffi::ffi::backingstore_error;
use crate::ffi::ffi::backingstore_error_with_code;

fn extract_remote_api_error(_err: &SaplingRemoteApiError) -> (BackingStoreErrorKind, Option<i32>) {
    (BackingStoreErrorKind::Generic, None)
}

fn extract_lfs_error(_err: &LfsFetchError) -> (BackingStoreErrorKind, Option<i32>) {
    (BackingStoreErrorKind::Generic, None)
}

fn extract_indexedlog_error(_err: &indexedlog::Error) -> BackingStoreErrorKind {
    BackingStoreErrorKind::Generic
}

/// Translate anyhow errors from the backinstore
/// to SaplingBackingStoreError in C++ for EdenFS to consume
pub(crate) fn into_backingstore_err(err: Error) -> UniquePtr<SaplingBackingStoreError> {
    let msg = format!("{:?}", err);
    let mut kind = BackingStoreErrorKind::Generic;
    let mut code: Option<i32> = None;
    for e in err.chain() {
        if let Some(e) = e.downcast_ref::<SaplingRemoteApiError>() {
            (kind, code) = extract_remote_api_error(e);
            break;
        } else if let Some(e) = e.downcast_ref::<LfsFetchError>() {
            (kind, code) = extract_lfs_error(e);
            break;
        } else if let Some(e) = e.downcast_ref::<indexedlog::Error>() {
            kind = extract_indexedlog_error(e);
            break;
        }
    }

    match code {
        Some(code) => backingstore_error_with_code(&msg, kind, code),
        None => backingstore_error(&msg, kind),
    }
}
