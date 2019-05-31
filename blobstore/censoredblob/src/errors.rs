// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure_ext::Fail;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Blob {} is censored and cannot be fetched.", _0)]
    Censored(String),
}
