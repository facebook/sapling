// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Contains structures describing configuration of the entire repo. Those structures are
//! deserialized from TOML files from metaconfig repo

use serde_derive::Deserialize;
use std::{
    collections::HashMap,
    convert::TryFrom,
    fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
    str,
    time::Duration,
};

use crate::errors::*;
use bookmarks::BookmarkName;
use failure_ext::ResultExt;
use metaconfig_types::{
    BlobConfig, BlobstoreId, BookmarkOrRegex, BookmarkParams, Bundle2ReplayParams,
    CacheWarmupParams, CommonConfig, HookBypass, HookConfig, HookManagerParams, HookParams,
    HookType, InfinitepushNamespace, InfinitepushParams, LfsParams, MetadataDBConfig,
    PushParams, PushrebaseParams, RepoConfig, RepoReadOnly,
    ShardedFilenodesParams, StorageConfig, WhitelistEntry,

};
use regex::Regex;
use toml;

const LIST_KEYS_PATTERNS_MAX_DEFAULT: u64 = 500_000;

/// Configuration of a metaconfig repository
#[derive(Debug, Eq, PartialEq)]
pub struct MetaConfig {}

/// Holds configuration all configuration that was read from metaconfig repository's manifest.
#[derive(Debug, PartialEq)]
pub struct RepoConfigs {
    /// Config for the config repository
    pub metaconfig: MetaConfig,
    /// Configs for all other repositories
    pub repos: HashMap<String, RepoConfig>,
    /// Common configs for all repos
    pub common: CommonConfig,
}

impl RepoConfigs {
    /// Read repo configs
    pub fn read_configs(config_path: impl AsRef<Path>) -> Result<Self> {
        let config_path = config_path.as_ref();

        let repos_dir = config_path.join("repos");
        if !repos_dir.is_dir() {
            return Err(ErrorKind::InvalidFileStructure(format!(
                "expected 'repos' directory under {}",
                config_path.display()
            ))
            .into());
        }
        let mut repo_configs = HashMap::new();
        for entry in repos_dir.read_dir()? {
            let entry = entry?;
            let dir_path = entry.path();
            if dir_path.is_dir() {
                let (name, config) =
                    RepoConfigs::read_single_repo_config(&dir_path, config_path)
                        .context(format!("while opening config for {:?} repo", dir_path))?;
                repo_configs.insert(name, config);
            }
        }

        let common_dir = config_path.join("common");
        let maybe_common_config = if common_dir.is_dir() {
            Self::read_common_config(&common_dir)?
        } else {
            None
        };

        let common = maybe_common_config.unwrap_or(Default::default());
        Ok(Self {
            metaconfig: MetaConfig {},
            repos: repo_configs,
            common,
        })
    }

    fn read_common_config(common_dir: &PathBuf) -> Result<Option<CommonConfig>> {
        for entry in common_dir.read_dir()? {
            let entry = entry?;
            if entry.file_name() == "common.toml" {
                let path = entry.path();
                if !path.is_file() {
                    return Err(ErrorKind::InvalidFileStructure(
                        "common/common.toml should be a file!".into(),
                    )
                    .into());
                }

                let content = fs::read(path)?;
                let raw_config = toml::from_slice::<RawCommonConfig>(&content)?;
                let mut tiers_num = 0;
                let whitelisted_entries: Result<Vec<_>> = raw_config
                    .whitelist_entry
                    .unwrap_or(vec![])
                    .into_iter()
                    .map(|whitelist_entry| {
                        let has_tier = whitelist_entry.tier.is_some();
                        let has_identity = {
                            if whitelist_entry.identity_data.is_none()
                                ^ whitelist_entry.identity_type.is_none()
                            {
                                return Err(ErrorKind::InvalidFileStructure(
                                    "identity type and data must be specified".into(),
                                )
                                .into());
                            }

                            whitelist_entry.identity_type.is_some()
                        };

                        if has_tier && has_identity {
                            return Err(ErrorKind::InvalidFileStructure(
                                "tier and identity cannot be both specified".into(),
                            )
                            .into());
                        }

                        if !has_tier && !has_identity {
                            return Err(ErrorKind::InvalidFileStructure(
                                "tier or identity must be specified".into(),
                            )
                            .into());
                        }

                        if whitelist_entry.tier.is_some() {
                            tiers_num += 1;
                            Ok(WhitelistEntry::Tier(whitelist_entry.tier.unwrap()))
                        } else {
                            let identity_type = whitelist_entry.identity_type.unwrap();

                            Ok(WhitelistEntry::HardcodedIdentity {
                                ty: identity_type,
                                data: whitelist_entry.identity_data.unwrap(),
                            })
                        }
                    })
                    .collect();

                if tiers_num > 1 {
                    return Err(
                        ErrorKind::InvalidFileStructure("only one tier is allowed".into()).into(),
                    );
                }
                return Ok(Some(CommonConfig {
                    security_config: whitelisted_entries?,
                }));
            }
        }
        Ok(None)
    }

    /// Read all common storage configurations
    pub fn read_storage_configs(
        config_root_path: impl AsRef<Path>,
    ) -> Result<HashMap<String, StorageConfig>> {
        let config_root_path = config_root_path.as_ref();

        Self::read_raw_storage_configs(config_root_path)?
            .into_iter()
            .map(|(k, v)| {
                StorageConfig::try_from(v)
                    .map(|v| (k, v))
                    .map_err(Error::from)
            })
            .collect()
    }

    fn read_raw_storage_configs(
        config_root_path: &Path,
    ) -> Result<HashMap<String, RawStorageConfig>> {
        let storage_config_path = config_root_path.join("common").join("storage.toml");
        let common_storage = if storage_config_path.exists() {
            if !storage_config_path.is_file() {
                return Err(ErrorKind::InvalidFileStructure(format!(
                    "invalid storage config path {:?}",
                    storage_config_path
                ))
                .into());
            } else {
                let content = fs::read(&storage_config_path)
                    .context(format!("While opening {}", storage_config_path.display()))?;
                toml::from_slice::<HashMap<String, RawStorageConfig>>(&content)
                    .context(format!("while reading {}", storage_config_path.display()))?
            }
        } else {
            HashMap::new()
        };

        Ok(common_storage)
    }

    fn read_single_repo_config(
        repo_config_path: &Path,
        config_root_path: &Path,
    ) -> Result<(String, RepoConfig)> {
        let common_storage = Self::read_raw_storage_configs(config_root_path)?;
        let reponame = repo_config_path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| {
                ErrorKind::InvalidFileStructure(format!("invalid repo path {:?}", repo_config_path))
            })?;
        let reponame = reponame.to_string();

        let config_file = repo_config_path.join("server.toml");
        if !config_file.is_file() {
            return Err(ErrorKind::InvalidFileStructure(format!(
                "expected file server.toml in {}",
                repo_config_path.to_string_lossy()
            ))
            .into());
        }

        let raw_config = toml::from_slice::<RawRepoConfig>(&fs::read(&config_file)?)?;

        let hooks = raw_config.hooks.clone();

        let mut all_hook_params = vec![];
        for raw_hook_config in hooks {
            let config = HookConfig {
                bypass: RepoConfigs::get_bypass(raw_hook_config.clone())?,
                strings: raw_hook_config.config_strings.unwrap_or_default(),
                ints: raw_hook_config.config_ints.unwrap_or_default(),
            };

            let hook_params = if raw_hook_config.name.starts_with("rust:") {
                // No need to load lua code for rust hook
                HookParams {
                    name: raw_hook_config.name,
                    code: None,
                    hook_type: raw_hook_config.hook_type,
                    config,
                }
            } else {
                let path = raw_hook_config.path.clone();
                let path = match path {
                    Some(path) => path,
                    None => {
                        return Err(ErrorKind::MissingPath().into());
                    }
                };
                let relative_prefix = "./";
                let is_relative = path.starts_with(relative_prefix);
                let path_adjusted = if is_relative {
                    let s: String = path.chars().skip(relative_prefix.len()).collect();
                    repo_config_path.join(s)
                } else {
                    config_root_path.join(path)
                };

                let contents = fs::read(&path_adjusted)
                    .context(format!("while reading hook {:?}", path_adjusted))?;
                let code = str::from_utf8(&contents)?;
                let code = code.to_string();
                HookParams {
                    name: raw_hook_config.name,
                    code: Some(code),
                    hook_type: raw_hook_config.hook_type,
                    config,
                }
            };

            all_hook_params.push(hook_params);
        }
        Ok((
            reponame,
            RepoConfigs::convert_conf(raw_config, common_storage, all_hook_params)?,
        ))
    }

    fn get_bypass(raw_hook_config: RawHookConfig) -> Result<Option<HookBypass>> {
        let bypass_commit_message = raw_hook_config
            .bypass_commit_string
            .map(|s| HookBypass::CommitMessage(s));

        let bypass_pushvar = raw_hook_config.bypass_pushvar.and_then(|s| {
            let pushvar: Vec<_> = s.split('=').map(|val| val.to_string()).collect();
            if pushvar.len() != 2 {
                return Some(Err(ErrorKind::InvalidPushvar(s).into()));
            }
            Some(Ok((
                pushvar.get(0).unwrap().clone(),
                pushvar.get(1).unwrap().clone(),
            )))
        });
        let bypass_pushvar = match bypass_pushvar {
            Some(Err(err)) => {
                return Err(err);
            }
            Some(Ok((name, value))) => Some(HookBypass::Pushvar { name, value }),
            None => None,
        };

        if bypass_commit_message.is_some() && bypass_pushvar.is_some() {
            return Err(ErrorKind::TooManyBypassOptions(raw_hook_config.name).into());
        }
        let bypass = bypass_commit_message.or(bypass_pushvar);

        Ok(bypass)
    }

    fn convert_conf(
        this: RawRepoConfig,
        common_storage: HashMap<String, RawStorageConfig>,
        hooks: Vec<HookParams>,
    ) -> Result<RepoConfig> {
        let mut common_storage = common_storage;
        let raw_storage_config = this
            .storage
            .get(&this.storage_config)
            .cloned()
            .or_else(|| common_storage.remove(&this.storage_config))
            .ok_or_else(|| {
                ErrorKind::InvalidConfig(format!("Storage \"{}\" not defined", this.storage_config))
            })?;

        let storage_config = StorageConfig::try_from(raw_storage_config)?;

        let enabled = this.enabled;
        let generation_cache_size = this.generation_cache_size.unwrap_or(10 * 1024 * 1024);
        let repoid = this.repoid;
        let scuba_table = this.scuba_table;
        let wireproto_scribe_category = this.wireproto_scribe_category;
        let cache_warmup = this.cache_warmup.map(|cache_warmup| CacheWarmupParams {
            bookmark: BookmarkName::new(cache_warmup.bookmark)
                .expect("bookmark name must be ascii"),
            commit_limit: cache_warmup.commit_limit.unwrap_or(200000),
        });
        let hook_manager_params = this.hook_manager_params.map(|params| HookManagerParams {
            entrylimit: params.entrylimit,
            weightlimit: params.weightlimit,
            disable_acl_checker: params.disable_acl_checker,
        });
        let bookmarks = {
            let mut bookmark_params = Vec::new();
            for bookmark in this.bookmarks.iter().cloned() {
                let bookmark_or_regex = match (bookmark.regex, bookmark.name) {
                    (None, Some(name)) => {
                        BookmarkOrRegex::Bookmark(BookmarkName::new(name).unwrap())
                    }
                    (Some(regex), None) => BookmarkOrRegex::Regex(regex.0),
                    _ => {
                        return Err(ErrorKind::InvalidConfig(
                            "bookmark's params need to specify regex xor name".into(),
                        )
                        .into());
                    }
                };

                let only_fast_forward = bookmark.only_fast_forward;
                let allowed_users = bookmark.allowed_users.map(|re| re.0);
                let rewrite_dates = bookmark.rewrite_dates;

                bookmark_params.push(BookmarkParams {
                    bookmark: bookmark_or_regex,
                    hooks: bookmark
                        .hooks
                        .into_iter()
                        .map(|rbmh| rbmh.hook_name)
                        .collect(),
                    only_fast_forward,
                    allowed_users,
                    rewrite_dates,
                });
            }
            bookmark_params
        };
        let bookmarks_cache_ttl = this.bookmarks_cache_ttl.map(Duration::from_millis);

        let push = this
            .push
            .map(|raw| {
                let default = PushParams::default();
                PushParams {
                    pure_push_allowed: raw.pure_push_allowed.unwrap_or(default.pure_push_allowed),
                }
            })
            .unwrap_or_default();

        let pushrebase = this
            .pushrebase
            .map(|raw| {
                let default = PushrebaseParams::default();
                PushrebaseParams {
                    rewritedates: raw.rewritedates.unwrap_or(default.rewritedates),
                    recursion_limit: raw.recursion_limit.unwrap_or(default.recursion_limit),
                    commit_scribe_category: raw.commit_scribe_category,
                    block_merges: raw.block_merges.unwrap_or(default.block_merges),
                    forbid_p2_root_rebases: raw
                        .forbid_p2_root_rebases
                        .unwrap_or(default.forbid_p2_root_rebases),
                    casefolding_check: raw.casefolding_check.unwrap_or(default.casefolding_check),
                    emit_obsmarkers: raw.emit_obsmarkers.unwrap_or(default.emit_obsmarkers),
                }
            })
            .unwrap_or_default();

        let bundle2_replay_params = this
            .bundle2_replay_params
            .map(|raw| Bundle2ReplayParams {
                preserve_raw_bundle2: raw.preserve_raw_bundle2.unwrap_or(false),
            })
            .unwrap_or_default();

        let lfs = match this.lfs {
            Some(lfs_params) => LfsParams {
                threshold: lfs_params.threshold,
            },
            None => LfsParams { threshold: None },
        };

        let hash_validation_percentage = this.hash_validation_percentage.unwrap_or(0);

        let readonly = if this.readonly {
            RepoReadOnly::ReadOnly("Set by config option".to_string())
        } else {
            RepoReadOnly::ReadWrite
        };

        let infinitepush = this.infinitepush.map(|p| {
            let namespace = InfinitepushNamespace::new(p.namespace.0);
            InfinitepushParams { namespace }
        });

        let list_keys_patterns_max = this
            .list_keys_patterns_max
            .unwrap_or(LIST_KEYS_PATTERNS_MAX_DEFAULT);

        let skiplist_index_blobstore_key = this.skiplist_index_blobstore_key;
        Ok(RepoConfig {
            enabled,
            storage_config,
            generation_cache_size,
            repoid,
            scuba_table,
            cache_warmup,
            hook_manager_params,
            bookmarks,
            bookmarks_cache_ttl,
            hooks,
            push,
            pushrebase,
            lfs,
            wireproto_scribe_category,
            hash_validation_percentage,
            readonly,
            skiplist_index_blobstore_key,
            bundle2_replay_params,
            write_lock_db_address: this.write_lock_db_address,
            infinitepush,
            list_keys_patterns_max,
        })
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct RawCommonConfig {
    whitelist_entry: Option<Vec<RawWhitelistEntry>>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct RawWhitelistEntry {
    tier: Option<String>,
    identity_data: Option<String>,
    identity_type: Option<String>,
}

fn is_true() -> bool {
    true
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct RawRepoConfig {
    /// Most important - the unique ID of this Repo
    repoid: i32,

    /// Persistent storage - contains location of metadata DB and name of blobstore we're
    /// using. We reference the common storage config by name.
    storage_config: String,

    /// Local definitions of storage (override the global set defined in storage.toml)
    #[serde(default)]
    storage: HashMap<String, RawStorageConfig>,

    /// Repo is enabled for use (default true)
    #[serde(default = "is_true")]
    enabled: bool,

    /// Repo is read-only (default false)
    #[serde(default)]
    readonly: bool,

    /// Define special bookmarks with parameters
    #[serde(default)]
    bookmarks: Vec<RawBookmarkConfig>,
    bookmarks_cache_ttl: Option<u64>,

    /// Define hook manager
    hook_manager_params: Option<HookManagerParams>,

    /// Define hook available for use on bookmarks
    #[serde(default)]
    hooks: Vec<RawHookConfig>,

    /// DB we're using for write-locking repos. This is separate from the rest because
    /// it's the same one Mercurial uses, to make it easier to manage repo locking for
    /// both from one tool.
    write_lock_db_address: Option<String>,

    // TODO: work out what these all are
    generation_cache_size: Option<usize>,
    scuba_table: Option<String>,
    delay_mean: Option<u64>,
    delay_stddev: Option<u64>,
    cache_warmup: Option<RawCacheWarmupConfig>,
    push: Option<RawPushParams>,
    pushrebase: Option<RawPushrebaseParams>,
    lfs: Option<RawLfsParams>,
    wireproto_scribe_category: Option<String>,
    hash_validation_percentage: Option<usize>,
    skiplist_index_blobstore_key: Option<String>,
    bundle2_replay_params: Option<RawBundle2ReplayParams>,
    infinitepush: Option<RawInfinitepushParams>,
    list_keys_patterns_max: Option<u64>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct RawCacheWarmupConfig {
    bookmark: String,
    commit_limit: Option<usize>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct RawHookManagerParams {
    entrylimit: usize,
    weightlimit: usize,
}

/// This structure helps to resolve an issue that when using serde_regex on Option<Regex> parsing
/// the toml file fails when the "regex" field is not provided. It works as expected when the
/// indirect Option<RawRegex> is used.
#[derive(Debug, Deserialize, Clone)]
struct RawRegex(#[serde(with = "serde_regex")] Regex);

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct RawBookmarkConfig {
    /// Either the regex or the name should be provided, not both
    regex: Option<RawRegex>,
    name: Option<String>,
    #[serde(default)]
    hooks: Vec<RawBookmarkHook>,
    // Are non fastforward moves allowed for this bookmark
    #[serde(default)]
    only_fast_forward: bool,
    /// Only users matching this pattern will be allowed to move this bookmark
    allowed_users: Option<RawRegex>,
    /// Whether or not to rewrite dates when processing pushrebase pushes
    rewrite_dates: Option<bool>,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct RawBookmarkHook {
    hook_name: String,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct RawHookConfig {
    name: String,
    path: Option<String>,
    hook_type: HookType,
    bypass_commit_string: Option<String>,
    bypass_pushvar: Option<String>,
    config_strings: Option<HashMap<String, String>>,
    config_ints: Option<HashMap<String, i32>>,
}

#[derive(Debug, Deserialize, Clone)]
//#[serde(deny_unknown_fields)] -- incompatible with flatten
struct RawStorageConfig {
    #[serde(rename = "db")]
    dbconfig: RawDbConfig,
    #[serde(flatten)]
    blobstore: RawBlobstoreConfig,
}

impl TryFrom<RawStorageConfig> for StorageConfig {
    type Error = Error;

    fn try_from(raw: RawStorageConfig) -> Result<StorageConfig> {
        let config = StorageConfig {
            dbconfig: match raw.dbconfig {
                RawDbConfig::Local { local_db_path } => MetadataDBConfig::LocalDB {
                    path: local_db_path,
                },
                RawDbConfig::Remote {
                    db_address,
                    sharded_filenodes: None,
                } => MetadataDBConfig::Mysql {
                    db_address,
                    sharded_filenodes: None,
                },
                RawDbConfig::Remote {
                    db_address,
                    sharded_filenodes:
                        Some(RawShardedFilenodesParams {
                            shard_map,
                            shard_num,
                        }),
                } => {
                    let shard_num: Result<_> = NonZeroUsize::new(shard_num).ok_or_else(|| {
                        ErrorKind::InvalidConfig("filenodes shard_num must be > 0".into()).into()
                    });
                    MetadataDBConfig::Mysql {
                        db_address,
                        sharded_filenodes: Some(ShardedFilenodesParams {
                            shard_map,
                            shard_num: shard_num?,
                        }),
                    }
                }
            },
            blobstore: TryFrom::try_from(&raw.blobstore)?,
        };
        Ok(config)
    }
}

/// Configuration for a single blobstore. These are intended to be defined in a
/// separate blobstore.toml config file, and then referenced by name from a per-server
/// config. Names are only necessary for blobstores which are going to be used by a server.
/// The id field identifies the blobstore as part of a multiplex, and need not be defined
/// otherwise. However, once it has been set for a blobstore, it must remain unchanged.
#[derive(Debug, Deserialize, Clone)]
#[serde(tag = "blobstore_type")]
#[serde(deny_unknown_fields)]
enum RawBlobstoreConfig {
    #[serde(rename = "disabled")]
    Disabled {}, // make this an empty struct so that deny_known_fields will complain
    #[serde(rename = "blob:files")]
    Files { path: PathBuf },
    #[serde(rename = "blob:rocks")]
    BlobRocks { path: PathBuf },
    #[serde(rename = "blob:sqlite")]
    BlobSqlite { path: PathBuf },
    #[serde(rename = "manifold")]
    Manifold {
        manifold_bucket: String,
        #[serde(default)]
        manifold_prefix: String,
    },
    #[serde(rename = "gluster")]
    Gluster {
        gluster_tier: String,
        gluster_export: String,
        gluster_basepath: String,
    },
    #[serde(rename = "mysql")]
    Mysql {
        mysql_shardmap: String,
        mysql_shard_num: i32,
    },
    #[serde(rename = "multiplexed")]
    Multiplexed {
        scuba_table: Option<String>,
        components: Vec<RawBlobstoreIdConfig>,
    },
}

impl TryFrom<&'_ RawBlobstoreConfig> for BlobConfig {
    type Error = Error;

    fn try_from(raw: &RawBlobstoreConfig) -> Result<BlobConfig> {
        let res = match raw {
            RawBlobstoreConfig::Disabled {} => BlobConfig::Disabled,
            RawBlobstoreConfig::Files { path } => BlobConfig::Files { path: path.clone() },
            RawBlobstoreConfig::BlobRocks { path } => BlobConfig::Rocks { path: path.clone() },
            RawBlobstoreConfig::BlobSqlite { path } => BlobConfig::Sqlite { path: path.clone() },
            RawBlobstoreConfig::Manifold {
                manifold_bucket,
                manifold_prefix,
            } => BlobConfig::Manifold {
                bucket: manifold_bucket.clone(),
                prefix: manifold_prefix.clone(),
            },
            RawBlobstoreConfig::Gluster {
                gluster_tier,
                gluster_export,
                gluster_basepath,
            } => BlobConfig::Gluster {
                tier: gluster_tier.clone(),
                export: gluster_export.clone(),
                basepath: gluster_basepath.clone(),
            },
            RawBlobstoreConfig::Mysql {
                mysql_shardmap,
                mysql_shard_num,
            } => BlobConfig::Mysql {
                shard_map: mysql_shardmap.clone(),
                shard_num: NonZeroUsize::new(*mysql_shard_num as usize).ok_or(
                    ErrorKind::InvalidConfig(
                        "mysql shard num must be specified and an interger larger than 0".into(),
                    ),
                )?,
            },
            RawBlobstoreConfig::Multiplexed {
                scuba_table,
                components,
            } => BlobConfig::Multiplexed {
                scuba_table: scuba_table.clone(),
                blobstores: components
                    .iter()
                    .map(|comp| Ok((comp.id, BlobConfig::try_from(&comp.blobstore)?)))
                    .collect::<Result<Vec<_>>>()?,
            },
        };
        Ok(res)
    }
}

#[derive(Debug, Deserialize, Clone)]
// #[serde(deny_unknown_fields)] -- incompatible with flatten
struct RawBlobstoreIdConfig {
    #[serde(rename = "blobstore_id")]
    id: BlobstoreId,
    #[serde(flatten)]
    blobstore: RawBlobstoreConfig,
}

/// Configuration for metadata db
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
#[serde(deny_unknown_fields)]
enum RawDbConfig {
    /// Specify base directory for a number of local DBs
    Local { local_db_path: PathBuf },
    /// Remote DB connection string where we use separate tables
    Remote {
        db_address: String,
        sharded_filenodes: Option<RawShardedFilenodesParams>,
    },
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPushParams {
    pure_push_allowed: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPushrebaseParams {
    rewritedates: Option<bool>,
    recursion_limit: Option<usize>,
    commit_scribe_category: Option<String>,
    block_merges: Option<bool>,
    forbid_p2_root_rebases: Option<bool>,
    casefolding_check: Option<bool>,
    emit_obsmarkers: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLfsParams {
    threshold: Option<u64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBundle2ReplayParams {
    preserve_raw_bundle2: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawShardedFilenodesParams {
    shard_map: String,
    shard_num: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawInfinitepushParams {
    namespace: RawRegex,
}

#[cfg(test)]
mod test {
    use super::*;
    use maplit::{btreemap, hashmap};
    use std::fs::{create_dir_all, write};
    use tempdir::TempDir;

    fn write_files(
        files: impl IntoIterator<Item = (impl AsRef<Path>, impl AsRef<[u8]>)>,
    ) -> TempDir {
        let tmp_dir = TempDir::new("mononoke_test_config").expect("tmp_dir failed");

        for (path, content) in files.into_iter() {
            let path = path.as_ref();
            let content = content.as_ref();

            let dir = path.parent().expect("missing parent");
            create_dir_all(tmp_dir.path().join(dir)).expect("create dir failed");
            write(tmp_dir.path().join(path), content).expect("write failed");
        }

        tmp_dir
    }

    #[test]
    fn test_read_manifest() {
        let hook1_content = "this is hook1";
        let hook2_content = "this is hook2";
        let fbsource_content = r#"
            write_lock_db_address="write_lock_db_address"
            generation_cache_size=1048576
            repoid=0
            scuba_table="scuba_table"
            skiplist_index_blobstore_key="skiplist_key"
            bookmarks_cache_ttl=5000
            storage_config="main"
            list_keys_patterns_max=123

            [cache_warmup]
            bookmark="master"
            commit_limit=100
            [hook_manager_params]
            entrylimit=1234
            weightlimit=4321
            disable_acl_checker=false

            [storage.main]
            db.db_address="db_address"
            db.sharded_filenodes = { shard_map = "db_address_shards", shard_num = 123 }

            blobstore_type="multiplexed"
            scuba_table = "blobstore_scuba_table"

                [[storage.main.components]]
                blobstore_id=0
                blobstore_type="manifold"
                manifold_bucket="bucket"

                [[storage.main.components]]
                blobstore_id=1
                blobstore_type="gluster"
                gluster_tier="mononoke.gluster.tier"
                gluster_export="groot"
                gluster_basepath="mononoke/glusterblob-test"

            [[bookmarks]]
            name="master"
            allowed_users="^(svcscm|twsvcscm)$"

            [[bookmarks.hooks]]
            hook_name="hook1"

            [[bookmarks.hooks]]
            hook_name="hook2"

            [[bookmarks.hooks]]
            hook_name="rust:rusthook"

            [[bookmarks]]
            regex="[^/]*/stable"

            [[hooks]]
            name="hook1"
            path="common/hooks/hook1.lua"
            hook_type="PerAddedOrModifiedFile"
            bypass_commit_string="@allow_hook1"

            [[hooks]]
            name="hook2"
            path="./hooks/hook2.lua"
            hook_type="PerChangeset"
            bypass_pushvar="pushvar=pushval"
            config_strings={ conf1 = "val1", conf2 = "val2" }

            [[hooks]]
            name="rust:rusthook"
            hook_type="PerChangeset"
            config_ints={ int1 = 44 }

            [push]
            pure_push_allowed = false

            [pushrebase]
            rewritedates = false
            recursion_limit = 1024
            forbid_p2_root_rebases = false
            casefolding_check = false
            emit_obsmarkers = false

            [lfs]
            threshold = 1000

            [bundle2_replay_params]
            preserve_raw_bundle2 = true

            [infinitepush]
            namespace = "foobar/.+"
        "#;
        let www_content = r#"
            repoid=1
            scuba_table="scuba_table"
            wireproto_scribe_category="category"
            storage_config="files"

            [storage.files]
            db.local_db_path = "/tmp/www"
            blobstore_type = "blob:files"
            path = "/tmp/www"
        "#;
        let common_content = r#"
            [[whitelist_entry]]
            tier = "tier1"

            [[whitelist_entry]]
            identity_type = "username"
            identity_data = "user"
        "#;

        let paths = btreemap! {
            "common/common.toml" => common_content,
            "common/hooks/hook1.lua" => hook1_content,
            "repos/fbsource/server.toml" => fbsource_content,
            "repos/fbsource/hooks/hook2.lua" => hook2_content,
            "repos/www/server.toml" => www_content,
            "my_path/my_files" => "",
        };

        let tmp_dir = write_files(&paths);

        let repoconfig = RepoConfigs::read_configs(tmp_dir.path()).expect("failed to read configs");

        let multiplex = BlobConfig::Multiplexed {
            scuba_table: Some("blobstore_scuba_table".to_string()),
            blobstores: vec![
                (
                    BlobstoreId::new(0),
                    BlobConfig::Manifold {
                        bucket: "bucket".into(),
                        prefix: "".into(),
                    },
                ),
                (
                    BlobstoreId::new(1),
                    BlobConfig::Gluster {
                        tier: "mononoke.gluster.tier".into(),
                        export: "groot".into(),
                        basepath: "mononoke/glusterblob-test".into(),
                    },
                ),
            ],
        };

        let mut repos = HashMap::new();
        repos.insert(
            "fbsource".to_string(),
            RepoConfig {
                enabled: true,
                storage_config: StorageConfig {
                    blobstore: multiplex,
                    dbconfig: MetadataDBConfig::Mysql {
                        db_address: "db_address".into(),
                        sharded_filenodes: Some(ShardedFilenodesParams {
                            shard_map: "db_address_shards".into(),
                            shard_num: NonZeroUsize::new(123).unwrap(),
                        }),
                    },
                },
                write_lock_db_address: Some("write_lock_db_address".into()),
                generation_cache_size: 1024 * 1024,
                repoid: 0,
                scuba_table: Some("scuba_table".to_string()),
                cache_warmup: Some(CacheWarmupParams {
                    bookmark: BookmarkName::new("master").unwrap(),
                    commit_limit: 100,
                }),
                hook_manager_params: Some(HookManagerParams {
                    entrylimit: 1234,
                    weightlimit: 4321,
                    disable_acl_checker: false,
                }),
                bookmarks_cache_ttl: Some(Duration::from_millis(5000)),
                bookmarks: vec![
                    BookmarkParams {
                        bookmark: BookmarkName::new("master").unwrap().into(),
                        hooks: vec![
                            "hook1".to_string(),
                            "hook2".to_string(),
                            "rust:rusthook".to_string(),
                        ],
                        only_fast_forward: false,
                        allowed_users: Some(Regex::new("^(svcscm|twsvcscm)$").unwrap()),
                        rewrite_dates: None,
                    },
                    BookmarkParams {
                        bookmark: Regex::new("[^/]*/stable").unwrap().into(),
                        hooks: vec![],
                        only_fast_forward: false,
                        allowed_users: None,
                        rewrite_dates: None,
                    },
                ],
                hooks: vec![
                    HookParams {
                        name: "hook1".to_string(),
                        code: Some("this is hook1".to_string()),
                        hook_type: HookType::PerAddedOrModifiedFile,
                        config: HookConfig {
                            bypass: Some(HookBypass::CommitMessage("@allow_hook1".into())),
                            strings: hashmap! {},
                            ints: hashmap! {},
                        },
                    },
                    HookParams {
                        name: "hook2".to_string(),
                        code: Some("this is hook2".to_string()),
                        hook_type: HookType::PerChangeset,
                        config: HookConfig {
                            bypass: Some(HookBypass::Pushvar {
                                name: "pushvar".into(),
                                value: "pushval".into(),
                            }),
                            strings: hashmap! {
                                "conf1".into() => "val1".into(),
                                "conf2".into() => "val2".into(),
                            },
                            ints: hashmap! {},
                        },
                    },
                    HookParams {
                        name: "rust:rusthook".to_string(),
                        code: None,
                        hook_type: HookType::PerChangeset,
                        config: HookConfig {
                            bypass: None,
                            strings: hashmap! {},
                            ints: hashmap! {
                                "int1".into() => 44,
                            },
                        },
                    },
                ],
                push: PushParams {
                    pure_push_allowed: false,
                },
                pushrebase: PushrebaseParams {
                    rewritedates: false,
                    recursion_limit: 1024,
                    commit_scribe_category: None,
                    block_merges: false,
                    forbid_p2_root_rebases: false,
                    casefolding_check: false,
                    emit_obsmarkers: false,
                },
                lfs: LfsParams {
                    threshold: Some(1000),
                },
                wireproto_scribe_category: None,
                hash_validation_percentage: 0,
                readonly: RepoReadOnly::ReadWrite,
                skiplist_index_blobstore_key: Some("skiplist_key".into()),
                bundle2_replay_params: Bundle2ReplayParams {
                    preserve_raw_bundle2: true,
                },
                infinitepush: Some(InfinitepushParams {
                    namespace: InfinitepushNamespace::new(Regex::new("foobar/.+").unwrap()),
                }),
                list_keys_patterns_max: 123,
            },
        );
        repos.insert(
            "www".to_string(),
            RepoConfig {
                enabled: true,
                storage_config: StorageConfig {
                    dbconfig: MetadataDBConfig::LocalDB {
                        path: "/tmp/www".into(),
                    },
                    blobstore: BlobConfig::Files {
                        path: "/tmp/www".into(),
                    },
                },
                write_lock_db_address: None,
                generation_cache_size: 10 * 1024 * 1024,
                repoid: 1,
                scuba_table: Some("scuba_table".to_string()),
                cache_warmup: None,
                hook_manager_params: None,
                bookmarks: vec![],
                bookmarks_cache_ttl: None,
                hooks: vec![],
                push: Default::default(),
                pushrebase: Default::default(),
                lfs: Default::default(),
                wireproto_scribe_category: Some("category".to_string()),
                hash_validation_percentage: 0,
                readonly: RepoReadOnly::ReadWrite,
                skiplist_index_blobstore_key: None,
                bundle2_replay_params: Bundle2ReplayParams::default(),
                infinitepush: None,
                list_keys_patterns_max: LIST_KEYS_PATTERNS_MAX_DEFAULT,
            },
        );
        assert_eq!(
            repoconfig.common,
            CommonConfig {
                security_config: vec![
                    WhitelistEntry::Tier("tier1".to_string()),
                    WhitelistEntry::HardcodedIdentity {
                        ty: "username".to_string(),
                        data: "user".to_string(),
                    },
                ],
            }
        );
        assert_eq!(
            repoconfig.repos.get("www"),
            repos.get("www"),
            "www mismatch\ngot {:#?}\nwant {:#?}",
            repoconfig.repos.get("www"),
            repos.get("www")
        );
        assert_eq!(
            repoconfig.repos.get("fbsource"),
            repos.get("fbsource"),
            "fbsource mismatch\ngot {:#?}\nwant {:#?}",
            repoconfig.repos.get("fbsource"),
            repos.get("fbsource")
        );

        assert_eq!(
            &repoconfig.repos, &repos,
            "Repo mismatch:\n\
             got:\n\
             {:#?}\n\
             Want:\n\
             {:#?}",
            repoconfig.repos, repos
        )
    }

    #[test]
    fn test_broken_bypass_config() {
        // Two bypasses for one hook
        let hook1_content = "this is hook1";
        let content = r#"
            repoid=0
            storage_config = "rocks"

            [storage.rocks]
            db.local_db_path = "/tmp/fbsource"
            blobstore_type = "blob:files"
            path = "/tmp/fbsource"

            [[bookmarks]]
            name="master"
            [[bookmarks.hooks]]
            hook_name="hook1"
            [[hooks]]
            name="hook1"
            path="common/hooks/hook1.lua"
            hook_type="PerAddedOrModifiedFile"
            bypass_commit_string="@allow_hook1"
            bypass_pushvar="var=val"
        "#;

        let paths = btreemap! {
            "common/hooks/hook1.lua" => hook1_content,
            "repos/fbsource/server.toml" => content,
        };

        let tmp_dir = write_files(&paths);

        let res = RepoConfigs::read_configs(tmp_dir.path());
        let msg = format!("{:?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("TooManyBypassOptions"));

        // Incorrect bypass string
        let hook1_content = "this is hook1";
        let content = r#"
            repoid=0
            storage_config = "rocks"

            [storage.rocks]
            db.local_db_path = "/tmp/fbsource"
            blobstore_type = "blob:files"
            path = "/tmp/fbsource"

            [[bookmarks]]
            name="master"
            [[bookmarks.hooks]]
            hook_name="hook1"
            [[hooks]]
            name="hook1"
            path="common/hooks/hook1.lua"
            hook_type="PerAddedOrModifiedFile"
            bypass_pushvar="var"
        "#;

        let paths = btreemap! {
            "common/hooks/hook1.lua" => hook1_content,
            "repos/fbsource/server.toml" => content,
        };

        let tmp_dir = write_files(&paths);

        let res = RepoConfigs::read_configs(tmp_dir.path());
        let msg = format!("{:?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("InvalidPushvar"));
    }

    #[test]
    fn test_broken_common_config() {
        fn check_fails(common: &str, expect: &str) {
            let content = r#"
                repoid = 0
                storage_config = "storage"

                [storage.storage]
                blobstore_type = "blob:rocks"
                path = "/tmp/fbsource"
                db.local_db_path = "/tmp/fbsource"
            "#;

            let paths = btreemap! {
                "common/common.toml" => common,
                "repos/fbsource/server.toml" => content,
            };

            let tmp_dir = write_files(&paths);

            let res = RepoConfigs::read_configs(tmp_dir.path());
            println!("res = {:?}", res);
            let msg = format!("{:?}", res);
            assert!(res.is_err(), "unexpected success for {}", common);
            assert!(
                msg.contains(expect),
                "wrong failure, wanted \"{}\" in {}",
                expect,
                common
            );
        }

        let common = r#"
        [[whitelist_entry]]
        identity_type="user"
        "#;
        check_fails(common, "identity type and data must be specified");

        let common = r#"
        [[whitelist_entry]]
        identity_data="user"
        "#;
        check_fails(common, "identity type and data must be specified");

        let common = r#"
        [[whitelist_entry]]
        tier="user"
        identity_type="user"
        identity_data="user"
        "#;
        check_fails(common, "tier and identity cannot be both specified");

        // Only one tier is allowed
        let common = r#"
        [[whitelist_entry]]
        tier="tier1"
        [[whitelist_entry]]
        tier="tier2"
        "#;
        check_fails(common, "only one tier is allowed");
    }

    #[test]
    fn test_common_storage() {
        const STORAGE: &str = r#"
        [multiplex_store]
        db.db_address = "some_db"
        db.sharded_filenodes = { shard_map="some-shards", shard_num=123 }

        blobstore_type = "multiplexed"
          [[multiplex_store.components]]
          blobstore_type = "gluster"
          blobstore_id = 1
          gluster_tier = "glustertier"
          gluster_export = "export"
          gluster_basepath = "groot"

        [manifold_store]
        db.db_address = "some_db"
        blobstore_type = "manifold"
        manifold_bucket = "bucketybucket"
        "#;

        const REPO: &str = r#"
        repoid = 123
        storage_config = "multiplex_store"

        # Not overriding common store
        [storage.some_other_store]
        db.db_address = "other_db"
        blobstore_type = "disabled"

        "#;

        let paths = btreemap! {
            "common/storage.toml" => STORAGE,
            "repos/test/server.toml" => REPO,
        };

        let tmp_dir = write_files(&paths);

        let res = RepoConfigs::read_configs(tmp_dir.path()).expect("read configs failed");

        let expected = hashmap! {
            "test".into() => RepoConfig {
                enabled: true,
                storage_config: StorageConfig {
                    blobstore: BlobConfig::Multiplexed {
                        scuba_table: None,
                        blobstores: vec![
                            (BlobstoreId::new(1), BlobConfig::Gluster {
                                tier: "glustertier".into(),
                                export: "export".into(),
                                basepath: "groot".into(),
                            })
                        ]
                    },
                    dbconfig: MetadataDBConfig::Mysql {
                        db_address: "some_db".into(),
                        sharded_filenodes: Some(ShardedFilenodesParams { shard_map: "some-shards".into(), shard_num: NonZeroUsize::new(123).unwrap()}),
                    },
                },
                repoid: 123,
                generation_cache_size: 10 * 1024 * 1024,
                list_keys_patterns_max: LIST_KEYS_PATTERNS_MAX_DEFAULT,
                ..Default::default()
            }
        };

        assert_eq!(
            res.repos, expected,
            "Got: {:#?}\nWant: {:#?}",
            &res.repos, expected
        )
    }

    #[test]
    fn test_common_blobstores_local_override() {
        const STORAGE: &str = r#"
        [multiplex_store]
        db.db_address = "some_db"
        blobstore_type = "multiplexed"
          [[multiplex_store.components]]
          blobstore_type = "gluster"
          blobstore_id = 1
          gluster_tier = "glustertier"
          gluster_export = "export"
          gluster_basepath = "groot"

        [manifold_store]
        db.db_address = "other_db"
        blobstore_type = "manifold"
        manifold_bucket = "bucketybucket"
        "#;

        const REPO: &str = r#"
        repoid = 123
        storage_config = "multiplex_store"

        # Override common store
        [storage.multiplex_store]
        db.db_address = "other_other_db"
        blobstore_type = "disabled"
        "#;

        let paths = btreemap! {
            "common/storage.toml" => STORAGE,
            "repos/test/server.toml" => REPO,
        };

        let tmp_dir = write_files(&paths);

        let res = RepoConfigs::read_configs(tmp_dir.path()).expect("read configs failed");

        let expected = hashmap! {
            "test".into() => RepoConfig {
                enabled: true,
                storage_config: StorageConfig {
                    blobstore: BlobConfig::Disabled,
                    dbconfig: MetadataDBConfig::Mysql {
                        db_address: "other_other_db".into(),
                        sharded_filenodes: None,
                    },
                },
                repoid: 123,
                generation_cache_size: 10 * 1024 * 1024,
                list_keys_patterns_max: LIST_KEYS_PATTERNS_MAX_DEFAULT,
                ..Default::default()
            }
        };

        assert_eq!(
            res.repos, expected,
            "Got: {:#?}\nWant: {:#?}",
            &res.repos, expected
        )
    }

    #[test]
    fn test_stray_fields() {
        const REPO: &str = r#"
        repoid = 123
        storage_config = "randomstore"

        [storage.randomstore]
        db.db_address = "other_other_db"
        blobstore_type = "files"
        path = "/tmp/foo"

        # Should be above
        readonly = true
        "#;

        let paths = btreemap! {
            "repos/test/server.toml" => REPO,
        };

        let tmp_dir = write_files(&paths);
        let err = RepoConfigs::read_configs(tmp_dir.path()).expect_err("read configs succeeded");
        println!("err = {:#?}", err);
    }

    #[test]
    fn test_stray_fields_disabled() {
        const REPO: &str = r#"
        repoid = 123
        storage_config = "disabled"

        [storage.disabled]
        db.db_address = "other_other_db"
        blobstore_type = "disabled"

        # Should be above
        readonly = true
        "#;

        let paths = btreemap! {
            "repos/test/server.toml" => REPO,
        };

        let tmp_dir = write_files(&paths);
        let err = RepoConfigs::read_configs(tmp_dir.path()).expect_err("read configs succeeded");
        println!("err = {:#?}", err);
    }
}
