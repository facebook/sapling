// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Scaffolding that's generally useful to build CLI tools on top of Mononoke.

#![deny(warnings)]
#![feature(never_type, bind_by_move_pattern_guards)]

pub mod args;
pub mod helpers;
mod log;
pub mod monitoring;
