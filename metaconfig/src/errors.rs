// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Definition of errors used in this crate by the error_chain crate

use std::str::Utf8Error;
use toml::de;
use vfs::errors as vfs_errors;

error_chain! {
    errors {
        /// The structure of metaconfig repo is invalid
        InvalidFileStructure(msg: String) {
            description("the structure of files in vfs is invalid")
            display("{}", msg)
        }
    }

    links {
        Vfs(vfs_errors::Error, vfs_errors::ErrorKind)
        #[doc = "Error originated in the vfs crate while manipulating the Vfs"];
    }

    foreign_links {
        De(de::Error) #[doc = "Failure in deserializing the config files"];
        Utf8(Utf8Error) #[doc = "Name of the repository is not in utf8"];
    }
}
