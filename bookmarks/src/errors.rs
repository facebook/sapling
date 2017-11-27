// Copyright (c) 2017-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

error_chain! {
    errors {
        IoError {
            description("The nested error represents an I/O issue")
        }
    }
}
