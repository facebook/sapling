// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.
use failure::Fail;

#[derive(Debug, Fail)]
#[fail(display = "invalid arguments\n(use '--help' to get help)")]
pub struct InvalidArguments;
