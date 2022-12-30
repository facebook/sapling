/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mononoke_app::subcommands! {
    mod blobstore;
    mod blobstore_unlink;
    mod bookmarks;
    mod changelog;
    mod commit;
    mod commit_graph;
    mod convert;
    mod fetch;
    mod filestore;
    mod hg_sync;
    mod mutable_renames;
    mod redaction;
    mod repo;
    mod repos;
    mod skiplist;
    mod ephemeral_store;
    mod dump_changesets;
    mod async_requests;
}
