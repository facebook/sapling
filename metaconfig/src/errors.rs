// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Definition of errors used in this crate by the error_chain crate

use vfs::errors as vfs_errors;

error_chain! {
    links {
        Vfs(vfs_errors::Error, vfs_errors::ErrorKind)
        #[doc = "Error originated in the vfs crate while manipulating the Vfs"];
    }
}
