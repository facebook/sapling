/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

fn main() {
    cc::Build::new()
        .file("../../edenscm/mercurial/mpatch.c")
        .include("../../")
        .compile("mpatch");
}
