// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure_ext::Fail;

use crate::expected_size::ExpectedSize;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Invalid size: {:?} was expected, {:?} was observed", _0, _1)]
    InvalidSize(ExpectedSize, u64),
}
