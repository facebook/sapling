// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]
// TODO: (sid0) T21726029 tokio/futures deprecated a bunch of stuff, clean it all up
#![allow(deprecated)]

extern crate futures;
extern crate mercurial_types;

pub mod manifest;
