// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Contains structures describing configuration of the entire repo. Those structures are
//! deserialized from TOML files from metaconfig repo

#![deny(missing_docs)]
#![deny(warnings)]

use std::{collections::HashMap, num::NonZeroUsize, path::PathBuf, str, sync::Arc, time::Duration};

use bookmarks::Bookmark;
use regex::Regex;
use scuba::ScubaValue;
use serde_derive::Deserialize;
use sql::mysql_async::{
    from_value_opt,
    prelude::{ConvIr, FromValue},
    FromValueError, Value,
};

/// Single entry that
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WhitelistEntry {
    /// Hardcoded whitelisted identity name i.e. USER (identity type) stash (identity data)
    HardcodedIdentity {
        /// Identity type
        ty: String,
        /// Identity data
        data: String,
    },
    /// Name of the tier
    Tier(String),
}

/// Configuration for all repos
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CommonConfig {
    /// Who can interact with Mononoke
    pub security_config: Vec<WhitelistEntry>,
}

/// Configuration of a single repository
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct RepoConfig {
    /// If false, this repo config is completely ignored.
    pub enabled: bool,
    /// Persistent storage for this repo
    pub storage_config: StorageConfig,
    /// Address of the SQL database used to lock writes to a repo.
    pub write_lock_db_address: Option<String>,
    /// How large a cache to use (in bytes) for RepoGenCache derived information
    pub generation_cache_size: usize,
    /// Numerical repo id of the repo.
    // XXX Use RepositoryId?
    pub repoid: i32,
    /// Scuba table for logging performance of operations
    pub scuba_table: Option<String>,
    /// Parameters of how to warm up the cache
    pub cache_warmup: Option<CacheWarmupParams>,
    /// Configuration for bookmarks
    pub bookmarks: Vec<BookmarkParams>,
    /// Enables bookmarks cache with specified ttl (time to live)
    pub bookmarks_cache_ttl: Option<Duration>,
    /// Configuration for hooks
    pub hooks: Vec<HookParams>,
    /// Pushrebase configuration options
    pub pushrebase: PushrebaseParams,
    /// LFS configuration options
    pub lfs: LfsParams,
    /// Scribe category to log all wireproto requests with full arguments.
    /// Used for replay on shadow tier.
    pub wireproto_scribe_category: Option<String>,
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
        self.storage_config.dbconfig.get_db_address()
    }
}

#[derive(Clone, PartialEq, Eq, Debug)]
/// Is the repo read-only?
pub enum RepoReadOnly {
    /// This repo is read-only and should not accept pushes or other writes
    ReadOnly(String),
    /// This repo should accept writes.
    ReadWrite,
}

impl Default for RepoReadOnly {
    fn default() -> Self {
        RepoReadOnly::ReadWrite
    }
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

impl BookmarkOrRegex {
    /// Checks whether a given Bookmark matches this bookmark or regex
    pub fn matches(&self, bookmark: &Bookmark) -> bool {
        match self {
            BookmarkOrRegex::Bookmark(ref bm) => bm.eq(bookmark),
            BookmarkOrRegex::Regex(ref re) => re.is_match(&bookmark.to_string()),
        }
    }
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

/// Collection of all bookmark attribtes
#[derive(Clone)]
pub struct BookmarkAttrs {
    bookmark_params: Arc<Vec<BookmarkParams>>,
}

impl BookmarkAttrs {
    /// create bookmark attributes from bookmark params vector
    pub fn new(bookmark_params: impl Into<Arc<Vec<BookmarkParams>>>) -> Self {
        Self {
            bookmark_params: bookmark_params.into(),
        }
    }

    /// select bookmark params matching provided bookmark
    pub fn select<'a>(
        &'a self,
        bookmark: &'a Bookmark,
    ) -> impl Iterator<Item = &'a BookmarkParams> {
        self.bookmark_params
            .iter()
            .filter(move |params| params.bookmark.matches(bookmark))
    }

    /// check if provided bookmark is fast-forward only
    pub fn is_fast_forward_only(&self, bookmark: &Bookmark) -> bool {
        self.select(bookmark).any(|params| params.only_fast_forward)
    }

    /// Check if a bookmark config overrides whether date should be rewritten during pushrebase.
    /// Return None if there are no bookmark config overriding rewrite_dates.
    pub fn should_rewrite_dates(&self, bookmark: &Bookmark) -> Option<bool> {
        for params in self.select(bookmark) {
            // NOTE: If there are multiple patterns matching the bookmark, the first match
            // overrides others. It might not be the most desired behavior, though.
            if let Some(rewrite_dates) = params.rewrite_dates {
                return Some(rewrite_dates);
            }
        }
        None
    }

    /// check if provided unix name is allowed to move specified bookmark
    pub fn is_allowed_user(&self, user: &Option<String>, bookmark: &Bookmark) -> bool {
        match user {
            None => true,
            Some(user) => {
                // NOTE: `Iterator::all` combinator returns `true` if selected set is empty
                //       which is consistent with what we want
                self.select(bookmark)
                    .flat_map(|params| &params.allowed_users)
                    .all(|re| re.is_match(user))
            }
        }
    }
}

/// Configuration for a bookmark
#[derive(Debug, Clone)]
pub struct BookmarkParams {
    /// The bookmark
    pub bookmark: BookmarkOrRegex,
    /// The hooks active for the bookmark
    pub hooks: Vec<String>,
    /// Are non fast forward moves blocked for this bookmark
    pub only_fast_forward: bool,
    /// Whether to rewrite dates for pushrebased commits or not
    pub rewrite_dates: Option<bool>,
    /// Only users matching this pattern will be allowed to move this bookmark
    pub allowed_users: Option<Regex>,
}

impl PartialEq for BookmarkParams {
    fn eq(&self, other: &Self) -> bool {
        let allowed_users_eq = match (&self.allowed_users, &other.allowed_users) {
            (None, None) => true,
            (Some(left), Some(right)) => left.as_str() == right.as_str(),
            _ => false,
        };
        allowed_users_eq
            && (self.bookmark == other.bookmark)
            && (self.hooks == other.hooks)
            && (self.only_fast_forward == other.only_fast_forward)
    }
}

impl Eq for BookmarkParams {}

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
    /// Block merge commits
    pub block_merges: bool,
    /// Forbid rebases when root is not a p1 of the rebase set.
    pub forbid_p2_root_rebases: bool,
    /// Whether to do chasefolding check during pushrebase
    pub casefolding_check: bool,
    /// Whether to do emit obsmarkers after pushrebase
    pub emit_obsmarkers: bool,
}

impl Default for PushrebaseParams {
    fn default() -> Self {
        PushrebaseParams {
            rewritedates: true,
            recursion_limit: 16384, // this number is fairly arbirary
            commit_scribe_category: None,
            block_merges: false,
            forbid_p2_root_rebases: true,
            casefolding_check: true,
            emit_obsmarkers: false,
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

/// Define storage needed for repo.
/// Storage consists of a blobstore and some kind of SQL DB for metadata. The configurations
/// can be broadly classified as "local" and "remote". "Local" is primarily for testing, and is
/// only suitable for single hosts. "Remote" is durable storage which can be shared by multiple
/// BlobRepo instances on different hosts.
#[derive(Debug, Default, Clone, Eq, PartialEq)]
pub struct StorageConfig {
    /// Blobstores. If the blobstore has a BlobstoreId then it can be used as a component of
    /// a Multiplexed blobstore.
    pub blobstore: BlobConfig,
    /// Metadata DB
    pub dbconfig: MetadataDBConfig,
}

/// Configuration for a blobstore
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum BlobConfig {
    /// Administratively disabled blobstore
    Disabled,
    /// Blob repository with path pointing to on-disk files with data. Blobs are stored in
    /// separate files.
    /// NOTE: this is read-only and for development/testing only. Production uses will break things.
    Files {
        /// Path to directory containing files
        path: PathBuf,
    },
    /// Blob repository with path pointing to on-disk files with data. The files are stored in a
    /// RocksDb database
    Rocks {
        /// Path to RocksDB directory
        path: PathBuf,
    },
    /// Blob repository with path pointing to on-disk files with data. The files are stored in a
    /// Sqlite database
    Sqlite {
        /// Path to SQLite DB
        path: PathBuf,
    },
    /// Store in a manifold bucket
    Manifold {
        /// Bucket of the backing Manifold blobstore to connect to
        bucket: String,
        /// Prefix to be prepended to all the keys. In prod it should be ""
        prefix: String,
    },
    /// Store in a gluster mount
    Gluster {
        /// Gluster tier
        tier: String,
        /// Nfs export name
        export: String,
        /// Content prefix path
        basepath: String,
    },
    /// Store in a sharded Mysql
    Mysql {
        /// Name of the Mysql shardmap to use
        shard_map: String,
        /// Number of shards in the Mysql shardmap
        shard_num: NonZeroUsize,
    },
    /// Multiplex across multiple blobstores for redundancy
    Multiplexed {
        /// A scuba table I guess
        scuba_table: Option<String>,
        /// Set of blobstores being multiplexed over
        blobstores: Vec<(BlobstoreId, BlobConfig)>,
    },
}

impl BlobConfig {
    /// Return true if the blobstore is strictly local. Multiplexed blobstores are local iff
    /// all their components are.
    pub fn is_local(&self) -> bool {
        use BlobConfig::*;

        match self {
            Disabled | Files { .. } | Rocks { .. } | Sqlite { .. } => true,
            Manifold { .. } | Gluster { .. } | Mysql { .. } => false,
            Multiplexed { blobstores, .. } => blobstores
                .iter()
                .map(|(_, config)| config)
                .all(BlobConfig::is_local),
        }
    }
}

impl Default for BlobConfig {
    fn default() -> Self {
        BlobConfig::Disabled
    }
}

/// Configuration for the Metadata DB
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum MetadataDBConfig {
    /// Remove MySQL DB
    Mysql {
        /// Identifies the SQL database to connect to.
        db_address: String,
        /// If present, sharding configuration for filenodes.
        sharded_filenodes: Option<ShardedFilenodesParams>,
    },
    /// Local SQLite dbs
    LocalDB {
        /// Path to directory of sqlite dbs
        path: PathBuf,
    },
}

impl Default for MetadataDBConfig {
    fn default() -> Self {
        MetadataDBConfig::LocalDB {
            path: PathBuf::default(),
        }
    }
}

impl MetadataDBConfig {
    /// Return true if this is a local on-disk DB.
    pub fn is_local(&self) -> bool {
        match self {
            MetadataDBConfig::LocalDB { .. } => true,
            MetadataDBConfig::Mysql { .. } => false,
        }
    }

    /// Return address we should connect to for a remote DB
    /// (Assumed to be Mysql)
    pub fn get_db_address(&self) -> Option<&str> {
        match self {
            MetadataDBConfig::Mysql { db_address, .. } => Some(db_address.as_str()),
            MetadataDBConfig::LocalDB { .. } => None,
        }
    }

    /// Return local path that stores local DB
    /// (Assumed to be Sqlite)
    pub fn get_local_address(&self) -> Option<&PathBuf> {
        match self {
            MetadataDBConfig::LocalDB { path } => Some(path),
            MetadataDBConfig::Mysql { .. } => None,
        }
    }
}

/// Params fro the bunle2 replay
#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct Bundle2ReplayParams {
    /// A flag specifying whether to preserve raw bundle2 contents in the blobstore
    pub preserve_raw_bundle2: bool,
}

/// Storage setup for sharded filenodes
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ShardedFilenodesParams {
    /// Identifies the SQL database to connect to.
    pub shard_map: String,
    /// Number of shards to distribute filenodes across.
    pub shard_num: NonZeroUsize,
}
