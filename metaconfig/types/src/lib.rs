// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Contains structures describing configuration of the entire repo. Those structures are
//! deserialized from TOML files from metaconfig repo

#![deny(missing_docs)]
#![deny(warnings)]

use bookmarks::Bookmark;
use regex::Regex;
use scuba::ScubaValue;
use serde_derive::Deserialize;
use sql::mysql_async::{
    from_value_opt,
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::str;

/// Arguments for setting up a Manifold blobstore.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifoldArgs {
    /// Bucket of the backing Manifold blobstore to connect to
    pub bucket: String,
    /// Prefix to be prepended to all the keys. In prod it should be ""
    pub prefix: String,
}

/// Arguments for settings up a Gluster blobstore
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlusterArgs {
    /// Gluster tier
    pub tier: String,
    /// Nfs export name
    pub export: String,
    /// Content prefix path
    pub basepath: String,
}

/// Arguments for setting up a Mysql blobstore.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MysqlBlobstoreArgs {
    /// Name of the Mysql shardmap to use
    pub shardmap: String,
    /// Number of shards in the Mysql shardmap
    pub shard_num: NonZeroUsize,
}

/// Configuration of a single repository
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct RepoConfig {
    /// If false, this repo config is completely ignored.
    pub enabled: bool,
    /// Defines the type of repository
    pub repotype: RepoType,
    /// How large a cache to use (in bytes) for RepoGenCache derived information
    pub generation_cache_size: usize,
    /// Numerical repo id of the repo.
    pub repoid: i32,
    /// Scuba table for logging performance of operations
    pub scuba_table: Option<String>,
    /// Parameters of how to warm up the cache
    pub cache_warmup: Option<CacheWarmupParams>,
    /// Configuration for bookmarks
    pub bookmarks: Option<Vec<BookmarkParams>>,
    /// Configuration for hooks
    pub hooks: Option<Vec<HookParams>>,
    /// Pushrebase configuration options
    pub pushrebase: PushrebaseParams,
    /// LFS configuration options
    pub lfs: LfsParams,
    /// Scribe category to log all wireproto requests with full arguments.
    /// Used for replay on shadow tier.
    pub wireproto_scribe_category: Option<String>,
    /// Scribe category to log all commits to.
    /// Used for triggering clone streamfile updates.
    pub post_commit_scribe_category: Option<String>,
    /// What percent of read request verifies that returned content matches the hash
    pub hash_validation_percentage: usize,
    /// Should this repo reject write attempts
    pub readonly: RepoReadOnly,
    /// Params for the hook manager
    pub hook_manager_params: Option<HookManagerParams>,
    /// Skiplist blobstore key (used to make revset faster)
    pub skiplist_index_blobstore_key: Option<String>,
    /// Params fro the bunle2 replay
    pub bundle2_replay_params: Bundle2ReplayParams,
}

impl RepoConfig {
    /// Returns a db address that is referenced in this config or None if there is none
    pub fn get_db_address(&self) -> Option<&str> {
        match self.repotype {
            RepoType::BlobRemote { ref db_address, .. } => Some(&db_address),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
/// Is the repo read-only?
pub enum RepoReadOnly {
    /// This repo is read-only and should not accept pushes or other writes
    ReadOnly,
    /// This repo should accept writes.
    ReadWrite,
}

/// Configuration of warming up the Mononoke cache. This warmup happens on startup
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CacheWarmupParams {
    /// Bookmark to warmup cache for at the startup. If not set then the cache will be cold.
    pub bookmark: Bookmark,
    /// Max number to fetch during commit warmup. If not set in the config, then set to a default
    /// value.
    pub commit_limit: usize,
}

/// Configuration for the hook manager
#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
pub struct HookManagerParams {
    /// Entry limit for the hook manager result cache
    pub entrylimit: usize,

    /// Weight limit for the hook manager result cache
    pub weightlimit: usize,

    /// Wether to disable the acl checker or not (intended for testing purposes)
    pub disable_acl_checker: bool,
}

impl Default for HookManagerParams {
    fn default() -> Self {
        Self {
            entrylimit: 1024 * 1024,
            weightlimit: 100 * 1024 * 1024, // 100Mb
            disable_acl_checker: false,
        }
    }
}

/// Configuration might be done for a single bookmark or for all bookmarks matching a regex
#[derive(Debug, Clone)]
pub enum BookmarkOrRegex {
    /// Matches a single bookmark
    Bookmark(Bookmark),
    /// Matches bookmarks with a regex
    Regex(Regex),
}

impl PartialEq for BookmarkOrRegex {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (BookmarkOrRegex::Bookmark(ref b1), BookmarkOrRegex::Bookmark(ref b2)) => b1.eq(b2),
            (BookmarkOrRegex::Regex(ref r1), BookmarkOrRegex::Regex(ref r2)) => {
                r1.as_str().eq(r2.as_str())
            }
            _ => false,
        }
    }
}
impl Eq for BookmarkOrRegex {}

impl From<Bookmark> for BookmarkOrRegex {
    fn from(b: Bookmark) -> Self {
        BookmarkOrRegex::Bookmark(b)
    }
}

impl From<Regex> for BookmarkOrRegex {
    fn from(r: Regex) -> Self {
        BookmarkOrRegex::Regex(r)
    }
}

/// Configuration for a bookmark
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct BookmarkParams {
    /// The bookmark
    pub bookmark: BookmarkOrRegex,
    /// The hooks active for the bookmark
    pub hooks: Option<Vec<String>>,
}

/// The type of the hook
#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
pub enum HookType {
    /// A hook that runs on the whole changeset
    PerChangeset,
    /// A hook that runs on a file in a changeset
    PerAddedOrModifiedFile,
}

/// Hook bypass
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum HookBypass {
    /// Bypass that checks that a string is in the commit message
    CommitMessage(String),
    /// Bypass that checks that a string is in the commit message
    Pushvar {
        /// Name of the pushvar
        name: String,
        /// Value of the pushvar
        value: String,
    },
}

/// Configs that are being passed to the hook during runtime
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct HookConfig {
    /// An optional way to bypass a hook
    pub bypass: Option<HookBypass>,
    /// Map of config to it's value. Values here are strings
    pub strings: HashMap<String, String>,
    /// Map of config to it's value. Values here are integers
    pub ints: HashMap<String, i32>,
}

/// Configuration for a hook
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct HookParams {
    /// The name of the hook
    pub name: String,
    /// The type of the hook
    pub hook_type: HookType,
    /// The code of the hook
    pub code: Option<String>,
    /// Configs that should be passed to hook
    pub config: HookConfig,
}

/// Pushrebase configuration options
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PushrebaseParams {
    /// Update dates of rebased commits
    pub rewritedates: bool,
    /// How far will we go from bookmark to find rebase root
    pub recursion_limit: usize,
    /// Scribe category we log new commits to
    pub commit_scribe_category: Option<String>,
}

impl Default for PushrebaseParams {
    fn default() -> Self {
        PushrebaseParams {
            rewritedates: true,
            recursion_limit: 16384, // this number is fairly arbirary
            commit_scribe_category: None,
        }
    }
}

/// LFS configuration options
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LfsParams {
    /// threshold in bytes, If None, Lfs is disabled
    pub threshold: Option<u64>,
}

impl Default for LfsParams {
    fn default() -> Self {
        LfsParams { threshold: None }
    }
}

/// Remote blobstore arguments
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RemoteBlobstoreArgs {
    /// Manifold arguments
    Manifold(ManifoldArgs),
    /// Gluster blobstore arguemnts
    Gluster(GlusterArgs),
    /// Mysql blobstore arguments
    Mysql(MysqlBlobstoreArgs),
    /// Multiplexed
    Multiplexed {
        /// Scuba table for tracking performance of blobstore operations
        scuba_table: Option<String>,
        /// Multiplexed blobstores
        blobstores: HashMap<BlobstoreId, RemoteBlobstoreArgs>,
    },
}

impl From<ManifoldArgs> for RemoteBlobstoreArgs {
    fn from(manifold_args: ManifoldArgs) -> Self {
        RemoteBlobstoreArgs::Manifold(manifold_args)
    }
}

impl From<GlusterArgs> for RemoteBlobstoreArgs {
    fn from(gluster_args: GlusterArgs) -> Self {
        RemoteBlobstoreArgs::Gluster(gluster_args)
    }
}

/// Id used to discriminate diffirent underlying blobstore instances
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Deserialize)]
pub struct BlobstoreId(u64);

impl BlobstoreId {
    /// Construct blobstore from integer
    pub fn new(id: u64) -> Self {
        BlobstoreId(id)
    }
}

impl From<BlobstoreId> for Value {
    fn from(id: BlobstoreId) -> Self {
        Value::UInt(id.0)
    }
}

impl ConvIr<BlobstoreId> for BlobstoreId {
    fn new(v: Value) -> std::result::Result<Self, FromValueError> {
        Ok(BlobstoreId(from_value_opt(v)?))
    }
    fn commit(self) -> Self {
        self
    }
    fn rollback(self) -> Value {
        self.into()
    }
}

impl FromValue for BlobstoreId {
    type Intermediate = BlobstoreId;
}

impl From<BlobstoreId> for ScubaValue {
    fn from(blobstore_id: BlobstoreId) -> Self {
        ScubaValue::from(blobstore_id.0 as i64)
    }
}

/// Types of repositories supported
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum RepoType {
    /// Blob repository with path pointing to on-disk files with data. The files are stored in a
    ///
    ///
    /// NOTE: this is read-only and for development/testing only. Production uses will break things.
    BlobFiles(PathBuf),
    /// Blob repository with path pointing to on-disk files with data. The files are stored in a
    /// RocksDb database
    BlobRocks(PathBuf),
    /// Blob repository with path pointing to on-disk files with data. The files are stored in a
    /// Sqlite database
    BlobSqlite(PathBuf),
    /// Blob repository with path pointing to the directory where a server socket is going to be.
    BlobRemote {
        /// Remote blobstores arguments
        blobstores_args: RemoteBlobstoreArgs,
        /// Identifies the SQL database to connect to.
        db_address: String,
        /// If present, the number of shards to spread filenodes across
        filenode_shards: Option<usize>,
    },
}

/// Params fro the bunle2 replay
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct Bundle2ReplayParams {
    /// A flag specifying whether to preserve raw bundle2 contents in the blobstore
    pub preserve_raw_bundle2: bool,
}
