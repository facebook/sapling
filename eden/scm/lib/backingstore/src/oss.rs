/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Open-source builds have no Rust bindings for folly::IOBuf, so this bridge
//! declares the type opaquely and creates one by copying bytes. It lives apart
//! from the main bridge in ffi.rs because Buck generates that bridge without
//! `--cfg fbcode_build`.

// TODO: T294824705: use folly's iobuf crate once it builds with Cargo.

use cxx::UniquePtr;

#[cxx::bridge(namespace = "sapling")]
mod ffi {
    unsafe extern "C++" {
        include!("eden/scm/lib/backingstore/include/iobuf_oss.h");

        #[namespace = "folly"]
        type IOBuf;

        fn sapling_backingstore_iobuf_copy_buffer(data: &[u8]) -> UniquePtr<IOBuf>;
    }
}

pub use ffi::IOBuf;

pub fn blob_into_iobuf(blob: blob::Blob) -> UniquePtr<IOBuf> {
    ffi::sapling_backingstore_iobuf_copy_buffer(&blob.into_bytes())
}
