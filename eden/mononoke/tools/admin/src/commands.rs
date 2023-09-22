/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Subcommands for `mononoke_newadmin`.
//!
//! The admin interface should be designed to be used by administrators of
//! Mononoke, who may not use the tool very frequently.  As such the commands
//! need to be discoverable.
//!
//! We should favour top-level commands that either:
//!
//! * Perform a specific task that is clear from their name (e.g.
//!   `dump-changesets`, `list-repos`).
//!
//! * Contain subcommands that apply to a clear concept within
//!   Mononoke (e.g. `locking`, `redaction`).
//!
//! Try to avoid top-level categories that are overly broad.  For example, a
//! `repo` or `repos` top-level command just hides commands and makes them
//! less discoverable: the vast majority of Mononoke commands operate on one
//! or more repos!

mononoke_app::subcommands! {
    mod async_requests;
    mod blobstore;
    mod blobstore_bulk_unlink;
    mod blobstore_unlink;
    mod bookmarks;
    mod changelog;
    mod commit;
    mod commit_graph;
    mod convert;
    mod derived_data;
    mod dump_changesets;
    mod ephemeral_store;
    mod fetch;
    mod filestore;
    mod git_bundle;
    mod git_symref;
    mod hg_sync;
    mod list_repos;
    mod locking;
    mod mutable_counters;
    mod mutable_renames;
    mod redaction;
    mod repo_info;
}
