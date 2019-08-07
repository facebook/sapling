// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure_ext::Error;

pub enum SubcommandError {
    InvalidArgs,
    Error(Error),
}

impl From<Error> for SubcommandError {
    fn from(err: Error) -> SubcommandError {
        SubcommandError::Error(err)
    }
}
