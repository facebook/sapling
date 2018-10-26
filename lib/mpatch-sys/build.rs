// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate cc;

fn main() {
    cc::Build::new()
        .file("../../mercurial/mpatch.c")
        .include("../../")
        .compile("mpatch");
}
