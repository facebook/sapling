// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Definition of errors used in this crate by the error_chain crate

use std::collections::VecDeque;

use mercurial_types::path::PathElement;

#[recursion_limit = "1024"]
error_chain! {
    errors {
        /// Tried to walk on a path that does not exists. Returns the remainder of walk.
        PathDoNotExists(msg: String, remainder: VecDeque<PathElement>) {
            description("the provided path does not exist in Vfs")
            display("{}", msg)
        }
        /// TODO(luk, T20453159) This is a temporary error, will be removed once all the
        /// functionalities of this library are finished
        NotImplemented(msg: String) {
            description("not implemented yet")
            display("{}", msg)
        }
        /// Reached maximum number of steps on the walk. Most commonly this happens when a symlink
        /// that leads into an infinite loop when resolved. Returns the remainder of walk.
        MaximumStepReached(msg: String, remainder: VecDeque<PathElement>) {
            description("maximum number of steps during a walk on Vfs was reached")
            display("{}", msg)
        }
    }
}
