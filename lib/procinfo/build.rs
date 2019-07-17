// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

fn main() {
    #[cfg(target_os = "macos")]
    {
        cc::Build::new().file("darwin.c").compile("darwin");
    }
}
