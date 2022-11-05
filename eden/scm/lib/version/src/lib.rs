/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub static VERSION: &'static str = match option_env!("SAPLING_VERSION") {
    Some(s) => s,
    None => "dev",
};
