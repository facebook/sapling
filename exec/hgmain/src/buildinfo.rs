/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(feature = "buildinfo")]
#[link(name = "buildinfo", kind = "static")]
extern "C" {
    pub fn print_buildinfo();
}
