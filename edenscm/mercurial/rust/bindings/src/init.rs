// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use env_logger;

/// Perform any initialization necessesary for Mercurial's Rust extensions.
pub(crate) fn init_rust() {
    env_logger::init();
}
