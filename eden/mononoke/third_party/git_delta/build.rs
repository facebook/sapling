/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

 fn main() {
    cc::Build::new()
        .files(&[
            "ffi/bridge.c",
            "original_sources/diff-delta.c",
        ])
        .includes(&[
            "original_sources/git-compat-util.h",
            "original_sources/banned.h",
            "original_sources/compat/bswap.h",
            "original_sources/delta.h",
            "original_sources/sane-ctype.h",
            "original_sources/wrapper.h",
        ])
        .std("c99")
        .flag("-DCARGO_BUILD=1")
        .flag("-DNO_OPENSSL=1")
        .compile("git_delta_bridge");
}
