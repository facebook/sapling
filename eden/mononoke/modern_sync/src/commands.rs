/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mononoke_app::subcommands! {
    mod benchmark;
    mod sync_loop;
    mod sync_once;
    mod sync_one;
    mod sync_sharded;
}
