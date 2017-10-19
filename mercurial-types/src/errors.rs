// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::convert::From;

use MPath;

#[recursion_limit = "1024"]
error_chain! {
    errors {
        InvalidSha1Input(msg: String) {
            description("invalid sha-1 input")
            display("invalid sha-1 input: {}", msg)
        }
        InvalidPath(path: Vec<u8>, msg: String) {
            description("invalid path")
            display("invalid path '{}': {}", String::from_utf8_lossy(&path[..]), msg)
        }
        InvalidMPath(path: MPath, msg: String) {
            description("invalid Mercurial path")
            display("invalid Mercurial path '{}', {}", path, msg)
        }
        InvalidFragmentList(msg: String) {
            description("invalid fragment list")
            display("invalid fragment list: {}", msg)
        }
    }

    foreign_links {
        Bincode(::bincode::Error);
        Utf8(::std::str::Utf8Error);
    }
}

impl From<!> for Error {
    fn from(_t: !) -> Error {
        unreachable!("never type cannot be instantiated")
    }
}
