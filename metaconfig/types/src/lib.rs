/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

//! Contains structures describing configuration of the entire repo. Those structures are
//! deserialized from TOML files from metaconfig repo

#![deny(missing_docs)]
#![deny(warnings)]

use anyhow::{anyhow, Error, Result};
use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto},
    fmt, mem,
    num::NonZeroUsize,
    path::PathBuf,
    str,
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use ascii::AsciiString;
use bookmarks::BookmarkName;
use mononoke_types::{MPath, RepositoryId};
use regex::Regex;
use repos::{
    RawBlobstoreConfig, RawDbConfig, RawFilestoreParams, RawShardedFilenodesParams,
    RawSourceControlServiceMonitoring, RawStorageConfig,
};
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
    /// Parent category to use for load limiting
    pub loadlimiter_category: Option<String>,
    /// Scuba table for logging redacted file accesses
    pub scuba_censored_table: Option<String>,
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
    pub repoid: RepositoryId,
    /// Scuba table for logging performance of operations
    pub scuba_table: Option<String>,
    /// Scuba table for logging hook executions
    pub scuba_table_hooks: Option<String>,
    /// Parameters of how to warm up the cache
    pub cache_warmup: Option<CacheWarmupParams>,
    /// Configuration for bookmarks
    pub bookmarks: Vec<BookmarkParams>,
    /// Infinitepush configuration
    pub infinitepush: InfinitepushParams,
    /// Enables bookmarks cache with specified ttl (time to live)
    pub bookmarks_cache_ttl: Option<Duration>,
    /// Configuration for hooks
    pub hooks: Vec<HookParams>,
    /// Push configuration options
    pub push: PushParams,
    /// Pushrebase configuration options
    pub pushrebase: PushrebaseParams,
    /// LFS configuration options
    pub lfs: LfsParams,
    /// Configuration for logging all wireproto requests with full arguments.
    /// Used for replay on shadow tier.
    pub wireproto_logging: WireprotoLoggingConfig,
    /// What percent of read request verifies that returned content matches the hash
    pub hash_validation_percentage: usize,
    /// Should this repo reject write attempts
    pub readonly: RepoReadOnly,
    /// Should files be checked for redaction
    pub redaction: Redaction,
    /// Params for the hook manager
    pub hook_manager_params: Option<HookManagerParams>,
    /// Skiplist blobstore key (used to make revset faster)
    pub skiplist_index_blobstore_key: Option<String>,
    /// Params fro the bunle2 replay
    pub bundle2_replay_params: Bundle2ReplayParams,
    /// Max number of results in listkeyspatterns.
    pub list_keys_patterns_max: u64,
    /// Params for File storage
    pub filestore: Option<FilestoreParams>,
    /// Config for commit sync
    pub commit_sync_config: Option<CommitSyncConfig>,
    /// Maximum size to consider files in hooks
    pub hook_max_file_size: u64,
    /// Hipster ACL that controls access to this repo
    pub hipster_acl: Option<String>,
    /// Configuration for the Source Control Service
    pub source_control_service: SourceControlServiceParams,
    /// Configuration for Source Control Service monitoring
    pub source_control_service_monitoring: Option<SourceControlServiceMonitoring>,
}

impl RepoConfig {
    /// Returns a db address that is referenced in this config or None if there is none
    pub fn get_db_address(&self) -> Option<String> {
        self.storage_config.dbconfig.get_db_address()
    }
}

#[derive(Eq, Copy, Clone, Debug, PartialEq, Deserialize)]
/// Should the redaction verification be enabled?
pub enum Redaction {
    /// Redacted files cannot be accessed
    Enabled,
    /// All the files can be fetched
    Disabled,
}

impl Default for Redaction {
    fn default() -> Self {
        Redaction::Enabled
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
    pub bookmark: BookmarkName,
    /// Max number to fetch during commit warmup. If not set in the config, then set to a default
    /// value.
    pub commit_limit: usize,
}

/// Configuration for the hook manager
#[derive(Debug, Clone, Eq, PartialEq, Deserialize)]
pub struct HookManagerParams {
    /// Wether to disable the acl checker or not (intended for testing purposes)
    pub disable_acl_checker: bool,
}

impl Default for HookManagerParams {
    fn default() -> Self {
        Self {
            disable_acl_checker: false,
        }
    }
}

/// Configuration might be done for a single bookmark or for all bookmarks matching a regex
#[derive(Debug, Clone)]
pub enum BookmarkOrRegex {
    /// Matches a single bookmark
    Bookmark(BookmarkName),
    /// Matches bookmarks with a regex
    Regex(Regex),
}

impl BookmarkOrRegex {
    /// Checks whether a given Bookmark matches this bookmark or regex
    pub fn matches(&self, bookmark: &BookmarkName) -> bool {
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

impl From<BookmarkName> for BookmarkOrRegex {
    fn from(b: BookmarkName) -> Self {
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
        bookmark: &'a BookmarkName,
    ) -> impl Iterator<Item = &'a BookmarkParams> {
        self.bookmark_params
            .iter()
            .filter(move |params| params.bookmark.matches(bookmark))
    }

    /// check if provided bookmark is fast-forward only
    pub fn is_fast_forward_only(&self, bookmark: &BookmarkName) -> bool {
        self.select(bookmark).any(|params| params.only_fast_forward)
    }

    /// Check if a bookmark config overrides whether date should be rewritten during pushrebase.
    /// Return None if there are no bookmark config overriding rewrite_dates.
    pub fn should_rewrite_dates(&self, bookmark: &BookmarkName) -> Option<bool> {
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
    pub fn is_allowed_user(&self, user: &Option<String>, bookmark: &BookmarkName) -> bool {
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

impl FromStr for HookType {
    type Err = Error;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        match string {
            "PerChangeset" => Ok(HookType::PerChangeset),
            "PerAddedOrModifiedFile" => Ok(HookType::PerAddedOrModifiedFile),
            _ => Err(anyhow!("Unable to parse {} as {}", string, "HookType")),
        }
    }
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

/// Push configuration options
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PushParams {
    /// Whether normal non-pushrebase pushes are allowed
    pub pure_push_allowed: bool,
}

impl Default for PushParams {
    fn default() -> Self {
        PushParams {
            pure_push_allowed: true,
        }
    }
}

/// Pushrebase configuration options
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct PushrebaseParams {
    /// Update dates of rebased commits
    pub rewritedates: bool,
    /// How far will we go from bookmark to find rebase root
    pub recursion_limit: Option<usize>,
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
            recursion_limit: Some(16384), // this number is fairly arbirary
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

impl fmt::Display for BlobstoreId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<BlobstoreId> for Value {
    fn from(id: BlobstoreId) -> Self {
        Value::UInt(id.0)
    }
}

impl ConvIr<BlobstoreId> for BlobstoreId {
    fn new(v: Value) -> Result<Self, FromValueError> {
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

impl TryFrom<RawStorageConfig> for StorageConfig {
    type Error = Error;

    fn try_from(raw: RawStorageConfig) -> Result<Self, Error> {
        let config = StorageConfig {
            dbconfig: match raw.db {
                RawDbConfig::local(def) => MetadataDBConfig::LocalDB {
                    path: PathBuf::from(def.local_db_path),
                },
                RawDbConfig::remote(def) => match def.sharded_filenodes {
                    None => MetadataDBConfig::Mysql {
                        db_address: def.db_address,
                        sharded_filenodes: None,
                    },
                    Some(RawShardedFilenodesParams {
                        shard_map,
                        shard_num,
                    }) => {
                        let shard_num: Result<_> = NonZeroUsize::new(shard_num.try_into()?)
                            .ok_or_else(|| anyhow!("filenodes shard_num must be > 0"));
                        MetadataDBConfig::Mysql {
                            db_address: def.db_address,
                            sharded_filenodes: Some(ShardedFilenodesParams {
                                shard_map,
                                shard_num: shard_num?,
                            }),
                        }
                    }
                },
                RawDbConfig::UnknownField(_) => {
                    return Err(anyhow!("unsupported storage configuration"));
                }
            },
            blobstore: TryFrom::try_from(&raw.blobstore)?,
        };
        Ok(config)
    }
}

/// What to do when the ScrubBlobstore finds a problem
#[derive(Debug, Clone, Copy, Eq, PartialEq, Deserialize)]
pub enum ScrubAction {
    /// Log items needing repair
    ReportOnly,
    /// Do repairs
    Repair,
}

impl FromStr for ScrubAction {
    type Err = Error;

    fn from_str(string: &str) -> Result<Self, Self::Err> {
        match string {
            "ReportOnly" => Ok(ScrubAction::ReportOnly),
            "Repair" => Ok(ScrubAction::Repair),
            _ => Err(anyhow!("Unable to parse {} as {}", string, "ScrubAction")),
        }
    }
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
    /// Multiplex across multiple blobstores scrubbing for errors
    Scrub {
        /// A scuba table I guess
        scuba_table: Option<String>,
        /// Set of blobstores being multiplexed over
        blobstores: Vec<(BlobstoreId, BlobConfig)>,
        /// Whether to attempt repair
        scrub_action: ScrubAction,
    },
    /// Store in a manifold bucket, but every object will have an expiration
    ManifoldWithTtl {
        /// Bucket of the backing Manifold blobstore to connect to
        bucket: String,
        /// Prefix to be prepended to all the keys. In prod it should be ""
        prefix: String,
        /// TTL for each object we put in Manifold
        ttl: Duration,
    },
}

impl BlobConfig {
    /// Return true if the blobstore is strictly local. Multiplexed blobstores are local iff
    /// all their components are.
    pub fn is_local(&self) -> bool {
        use BlobConfig::*;

        match self {
            Disabled | Files { .. } | Rocks { .. } | Sqlite { .. } => true,
            Manifold { .. } | Mysql { .. } | ManifoldWithTtl { .. } => false,
            Multiplexed { blobstores, .. } | Scrub { blobstores, .. } => blobstores
                .iter()
                .map(|(_, config)| config)
                .all(BlobConfig::is_local),
        }
    }

    /// Change all internal blobstores to scrub themselves for errors where possible.
    /// This maximises error rates, and asks blobstores to silently fix errors when they are able
    /// to do so - ideal for repository checkers.
    pub fn set_scrubbed(&mut self, scrub_action: ScrubAction) {
        use BlobConfig::{Multiplexed, Scrub};

        if let Multiplexed {
            scuba_table,
            blobstores,
        } = self
        {
            let scuba_table = mem::replace(scuba_table, None);
            let mut blobstores = mem::replace(blobstores, Vec::new());
            for (_, store) in blobstores.iter_mut() {
                store.set_scrubbed(scrub_action);
            }
            *self = Scrub {
                scuba_table,
                blobstores,
                scrub_action,
            };
        }
    }
}

impl Default for BlobConfig {
    fn default() -> Self {
        BlobConfig::Disabled
    }
}

impl TryFrom<&'_ RawBlobstoreConfig> for BlobConfig {
    type Error = Error;

    fn try_from(raw: &RawBlobstoreConfig) -> Result<Self, Error> {
        let res = match raw {
            RawBlobstoreConfig::disabled(_) => BlobConfig::Disabled,
            RawBlobstoreConfig::blob_files(def) => BlobConfig::Files {
                path: PathBuf::from(def.path.clone()),
            },
            RawBlobstoreConfig::blob_rocks(def) => BlobConfig::Rocks {
                path: PathBuf::from(def.path.clone()),
            },
            RawBlobstoreConfig::blob_sqlite(def) => BlobConfig::Sqlite {
                path: PathBuf::from(def.path.clone()),
            },
            RawBlobstoreConfig::manifold(def) => BlobConfig::Manifold {
                bucket: def.manifold_bucket.clone(),
                prefix: def.manifold_prefix.clone(),
            },
            RawBlobstoreConfig::mysql(def) => BlobConfig::Mysql {
                shard_map: def.mysql_shardmap.clone(),
                shard_num: NonZeroUsize::new(def.mysql_shard_num.try_into()?).ok_or(anyhow!(
                    "mysql shard num must be specified and an interger larger than 0"
                ))?,
            },
            RawBlobstoreConfig::multiplexed(def) => BlobConfig::Multiplexed {
                scuba_table: def.scuba_table.clone(),
                blobstores: def
                    .components
                    .iter()
                    .map(|comp| {
                        Ok((
                            BlobstoreId(comp.blobstore_id.try_into()?),
                            BlobConfig::try_from(&comp.blobstore)?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?,
            },
            RawBlobstoreConfig::manifold_with_ttl(def) => {
                let ttl = Duration::from_secs(def.ttl_secs.try_into()?);
                BlobConfig::ManifoldWithTtl {
                    bucket: def.manifold_bucket.clone(),
                    prefix: def.manifold_prefix.clone(),
                    ttl,
                }
            }
            RawBlobstoreConfig::UnknownField(_) => {
                return Err(anyhow!("unsupported blobstore configuration"));
            }
        };
        Ok(res)
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
    pub fn get_db_address(&self) -> Option<String> {
        match self {
            MetadataDBConfig::Mysql { db_address, .. } => Some(db_address.clone()),
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

/// Regex for valid branches that Infinite Pushes can be directed to.
#[derive(Debug, Clone)]
pub struct InfinitepushNamespace(Regex);

impl PartialEq for InfinitepushNamespace {
    fn eq(&self, other: &Self) -> bool {
        self.0.as_str() == other.0.as_str()
    }
}

impl Eq for InfinitepushNamespace {}

impl InfinitepushNamespace {
    /// Instantiate a new InfinitepushNamespace
    pub fn new(regex: Regex) -> Self {
        Self(regex)
    }

    /// Returns whether a given Bookmark matches this namespace.
    pub fn matches_bookmark(&self, bookmark: &BookmarkName) -> bool {
        self.0.is_match(bookmark.as_str())
    }

    /// Returns this namespace's controlling Regex.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

/// Infinitepush configuration. Note that it is legal to not allow Infinitepush (server = false),
/// while still providing a namespace. Doing so will prevent regular pushes to the namespace, as
/// well as allow the creation of Infinitepush scratchbookmarks through e.g. replicating them from
/// Mercurial.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InfinitepushParams {
    /// Whether infinite push bundles are allowed on this server. If false, all infinitepush
    /// bundles will be rejected.
    pub allow_writes: bool,

    /// Valid namespace for infinite push bookmarks. If None, then infinitepush bookmarks are not
    /// allowed.
    pub namespace: Option<InfinitepushNamespace>,
}

impl Default for InfinitepushParams {
    fn default() -> Self {
        Self {
            allow_writes: false,
            namespace: None,
        }
    }
}

/// Filestore configuration.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct FilestoreParams {
    /// Chunk size for the Filestore, in bytes.
    pub chunk_size: u64,
    /// Max number of concurrent chunk uploads to perform in the Filestore.
    pub concurrency: usize,
}

/// Default path action to perform when syncing commits
/// from small to large repos
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum DefaultSmallToLargeCommitSyncPathAction {
    /// Preserve as is
    Preserve,
    /// Prepend a given prefix to the path
    PrependPrefix(MPath),
}

/// Commit sync configuration for a small repo
/// Note: this configuration is always from the point of view
/// of the small repo, meaning a key in the `map` is a path
/// prefix in the small repo, and a value - in the large repo
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SmallRepoCommitSyncConfig {
    /// Default action to take on a path
    pub default_action: DefaultSmallToLargeCommitSyncPathAction,
    /// A map of prefix replacements when syncing
    pub map: HashMap<MPath, MPath>,
    /// Bookmark prefix to use in the large repo
    pub bookmark_prefix: AsciiString,
    /// Commit sync direction
    pub direction: CommitSyncDirection,
}

/// Commit sync direction
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum CommitSyncDirection {
    /// Syncing commits from large repo to small ones
    LargeToSmall,
    /// Syncing commits from small repos to large one
    SmallToLarge,
}

/// Commit sync configuration for a large repo
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CommitSyncConfig {
    /// Large repository id
    pub large_repo_id: RepositoryId,
    /// Common pushrebase bookmarks
    pub common_pushrebase_bookmarks: Vec<BookmarkName>,
    /// Corresponding small repo configs
    pub small_repos: HashMap<RepositoryId, SmallRepoCommitSyncConfig>,
}

/// Configuration for logging wireproto commands and arguments
/// This is used by traffic replay script to replay on prod traffic on shadow tier
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct WireprotoLoggingConfig {
    /// Scribe category to log to
    pub scribe_category: Option<String>,
    /// Storage config to store wireproto arguments. The arguments can be quite big,
    /// so storing separately would make sense.
    /// Second parameter is threshold. If wireproto arguments are bigger than this threshold
    /// then they will be stored in remote storage defined by first parameter. Note that if
    /// `storage_config_and_threshold` is not specified then wireproto wireproto arguments will
    /// be inlined
    pub storage_config_and_threshold: Option<(StorageConfig, u64)>,
}

impl WireprotoLoggingConfig {
    /// Create WireprotoLoggingConfig with correct default values
    pub fn new(
        scribe_category: Option<String>,
        storage_config_and_threshold: Option<(StorageConfig, u64)>,
    ) -> Self {
        Self {
            scribe_category,
            storage_config_and_threshold,
        }
    }
}

impl Default for WireprotoLoggingConfig {
    fn default() -> Self {
        Self {
            scribe_category: None,
            storage_config_and_threshold: None,
        }
    }
}

/// Source Control Service options
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SourceControlServiceParams {
    /// whether writes are permitted
    pub permit_writes: bool,
}

impl Default for SourceControlServiceParams {
    fn default() -> Self {
        SourceControlServiceParams {
            permit_writes: false,
        }
    }
}

/// Configuration for health monitoring of the Source Control Service
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct SourceControlServiceMonitoring {
    /// Bookmarks, for which we want our services to log
    /// age values to monitoring counters. For example,
    /// a freshness value may be the `now - author_date` of
    /// the commit, to which the bookmark points
    pub bookmarks_to_report_age: Vec<BookmarkName>,
}

impl TryFrom<RawFilestoreParams> for FilestoreParams {
    type Error = Error;

    fn try_from(raw: RawFilestoreParams) -> Result<Self, Error> {
        let RawFilestoreParams {
            chunk_size,
            concurrency,
        } = raw;

        Ok(FilestoreParams {
            chunk_size: chunk_size.try_into()?,
            concurrency: concurrency.try_into()?,
        })
    }
}

impl TryFrom<RawSourceControlServiceMonitoring> for SourceControlServiceMonitoring {
    type Error = Error;

    fn try_from(t: RawSourceControlServiceMonitoring) -> Result<Self, Error> {
        let bookmarks_to_report_age = t
            .bookmarks_to_report_age
            .into_iter()
            .map(|bookmark| BookmarkName::new(bookmark))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(SourceControlServiceMonitoring {
            bookmarks_to_report_age,
        })
    }
}
