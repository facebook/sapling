/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mononoke_app::subcommands! {
    mod blobstore;
    mod blobstore_unlink;
    mod fetch;
    mod list_repos;
    mod repo_info;
}
