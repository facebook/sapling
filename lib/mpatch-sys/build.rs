// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

fn main() {
    cc::Build::new()
        .file("../../edenscm/mercurial/mpatch.c")
        .include("../../")
        .compile("mpatch");
}
