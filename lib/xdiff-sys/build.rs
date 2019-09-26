// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

fn main() {
    cc::Build::new()
        .files(&[
            "../third-party/xdiff/xdiffi.c",
            "../third-party/xdiff/xprepare.c",
            "../third-party/xdiff/xutils.c",
        ])
        .compile("xdiff");
}
