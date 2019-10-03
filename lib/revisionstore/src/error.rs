// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use failure::Fail;

#[derive(Debug, Fail)]
#[fail(display = "Empty Mutable Pack")]
pub struct EmptyMutablePack();
