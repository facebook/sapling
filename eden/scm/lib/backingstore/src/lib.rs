/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! backingstore - The crate provides backing store interface for EdenFS.
//!
//! This crate aims to provide EdenFS's backing store interface so EdenFS could use types in this
//! crate to import SCM blobs and trees directly from Mercurial's data store.
//!
//! The C++ code in `c_api` directory encapsulate Rust functions exposed from this crate into
//! regular C++ classes.
//!
//! Changes to this create may need regeneration of the C/C++ binding header.
//! To regenerate the binding header, run `./tools/cbindgen.sh`.

mod auxdata;
mod backingstore;
mod ffi;
mod init;
mod prefetch;
mod request;
mod tree;

pub use crate::backingstore::BackingStore;
pub use crate::init::backingstore_global_init;
