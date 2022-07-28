/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::ScscApp;

base_app::subcommands! {
    type App = ScscApp;
    mod cat;
    mod blame;
    mod common_base;
    mod diff;
    mod export;
    mod find_files;
    mod info;
    mod is_ancestor;
    mod list_bookmarks;
    mod log;
    mod lookup;
    mod ls;
    mod pushrebase_history;
    mod repos;
    mod run_hooks;
}
