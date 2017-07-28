// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#[recursion_limit = "1024"]
error_chain! {
    errors {
        InvalidSha1Input(msg: String) {
            description("invalid sha-1 input")
            display("invalid sha-1 input: {}", msg)
        }
        InvalidPath(msg: String) {
            description("invalid path")
            display("invalid path: {}", msg)
        }
        InvalidFragmentList(msg: String) {
            description("invalid fragment list")
            display("invalid fragment list: {}", msg)
        }
    }

    foreign_links {
        Utf8(::std::str::Utf8Error);
    }
}
