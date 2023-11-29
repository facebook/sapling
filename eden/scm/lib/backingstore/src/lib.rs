/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
mod request;
mod tree;

pub use crate::backingstore::BackingStore;

#[derive(Debug, Copy, Clone)]
pub enum FetchMode {
    /// The fetch may hit remote servers.
    AllowRemote,
    /// The fetch is limited to RAM and disk.
    LocalOnly,
}

impl FetchMode {
    pub fn is_local(self) -> bool {
        matches!(self, FetchMode::LocalOnly)
    }

    pub fn from_local(local: bool) -> Self {
        if local {
            Self::LocalOnly
        } else {
            Self::AllowRemote
        }
    }
}
