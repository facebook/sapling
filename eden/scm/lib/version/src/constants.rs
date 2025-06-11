/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub static VERSION: &str = match option_env!("SAPLING_VERSION") {
    Some(s) => s,
    None => "dev",
};

pub static VERSION_HASH: &str = match option_env!("SAPLING_VERSION_HASH") {
    Some(s) => s,
    None => "",
};
