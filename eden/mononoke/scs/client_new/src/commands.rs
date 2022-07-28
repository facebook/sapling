/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ScscApp;

base_app::subcommands! {
    mod cat;
    mod repos;
    mod blame;
    mod common_base;
    mod diff;
    mod export;
    mod info;
    mod find_files;
    app = ScscApp
}
