/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clap::ArgEnum;
use clap::ArgGroup;
use clap::Args;

/// Command line arguments for specifying a blobstore, either by
/// repo, or by storage name.
#[derive(Args, Debug)]
#[clap(group(
    ArgGroup::new("repo_blobstore")
        .required(true)
        .args(&["repo-id", "repo-name", "storage-name"]),
))]
pub struct RepoBlobstoreArgs {
    /// Numeric repository ID
    #[clap(long)]
    pub repo_id: Option<i32>,

    /// Repository name
    #[clap(short = 'R', long)]
    pub repo_name: Option<String>,

    /// Storage name
    #[clap(long)]
    pub storage_name: Option<String>,

    /// If the blobstore is multiplexed, use this inner blobstore
    #[clap(long)]
    pub inner_blobstore_id: Option<u64>,

    /// Use memcache to cache access to the blobstore
    #[clap(long, arg_enum)]
    pub use_memcache: Option<UseMemcache>,

    /// Don't prepend the repo prefix to the key
    #[clap(long)]
    pub no_prefix: bool,
}

#[derive(Copy, Clone, Debug, ArgEnum, Eq, PartialEq)]
pub enum UseMemcache {
    CacheOnly,
    NoFill,
    FillMc,
}
