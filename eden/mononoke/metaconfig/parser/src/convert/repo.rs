/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::str::FromStr;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use bookmarks_types::BookmarkKey;
use metaconfig_types::Address;
use metaconfig_types::BlameVersion;
use metaconfig_types::BookmarkOrRegex;
use metaconfig_types::BookmarkParams;
use metaconfig_types::CacheWarmupParams;
use metaconfig_types::CommitCloudConfig;
use metaconfig_types::CommitGraphConfig;
use metaconfig_types::CommitIdentityScheme;
use metaconfig_types::ComparableRegex;
use metaconfig_types::CrossRepoCommitValidation;
use metaconfig_types::DerivedDataConfig;
use metaconfig_types::DerivedDataTypesConfig;
use metaconfig_types::GitBundleURIConfig;
use metaconfig_types::GitConcurrencyParams;
use metaconfig_types::GitConfigs;
use metaconfig_types::GitDeltaManifestV2Config;
use metaconfig_types::GitDeltaManifestV3Config;
use metaconfig_types::GitDeltaManifestVersion;
use metaconfig_types::GlobalrevConfig;
use metaconfig_types::HookBypass;
use metaconfig_types::HookConfig;
use metaconfig_types::HookManagerParams;
use metaconfig_types::HookParams;
use metaconfig_types::InferredCopyFromConfig;
use metaconfig_types::InfinitepushNamespace;
use metaconfig_types::InfinitepushParams;
use metaconfig_types::LfsParams;
use metaconfig_types::LoggingDestination;
use metaconfig_types::MetadataCacheConfig;
use metaconfig_types::MetadataCacheUpdateMode;
use metaconfig_types::MetadataLoggerConfig;
use metaconfig_types::ModernSyncChannelConfig;
use metaconfig_types::ModernSyncConfig;
use metaconfig_types::MononokeCasSyncConfig;
use metaconfig_types::PushParams;
use metaconfig_types::PushrebaseFlags;
use metaconfig_types::PushrebaseParams;
use metaconfig_types::PushrebaseRemoteMode;
use metaconfig_types::RemoteDerivationConfig;
use metaconfig_types::RepoClientKnobs;
use metaconfig_types::ServiceWriteRestrictions;
use metaconfig_types::ShardedService;
use metaconfig_types::ShardingModeConfig;
use metaconfig_types::SourceControlServiceMonitoring;
use metaconfig_types::SourceControlServiceParams;
use metaconfig_types::SparseProfilesConfig;
use metaconfig_types::UnodeVersion;
use metaconfig_types::UpdateLoggingConfig;
use metaconfig_types::UriGeneratorType;
use metaconfig_types::WalkerConfig;
use metaconfig_types::WalkerJobParams;
use metaconfig_types::WalkerJobType;
use metaconfig_types::XRepoSyncSourceConfig;
use metaconfig_types::XRepoSyncSourceConfigMapping;
use metaconfig_types::ZelosConfig;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mononoke_types::NonRootMPath;
use mononoke_types::PrefixTrie;
use mononoke_types::RepositoryId;
use mononoke_types::path::MPath;
use regex::Regex;
use repos::ModernSyncChannelConfig as RawModernSyncChannelConfig;
use repos::RawBookmarkConfig;
use repos::RawCacheWarmupConfig;
use repos::RawCasSyncConfig;
use repos::RawCommitCloudConfig;
use repos::RawCommitGraphConfig;
use repos::RawCommitIdentityScheme;
use repos::RawCrossRepoCommitValidationConfig;
use repos::RawDerivedDataBlockedChangesetDerivation;
use repos::RawDerivedDataBlockedDerivation;
use repos::RawDerivedDataConfig;
use repos::RawDerivedDataTypesConfig;
use repos::RawGitBundleURIConfig;
use repos::RawGitConcurrencyParams;
use repos::RawGitConfigs;
use repos::RawGitDeltaManifestV2Config;
use repos::RawGitDeltaManifestV3Config;
use repos::RawHookConfig;
use repos::RawHookManagerParams;
use repos::RawInferredCopyFromConfig;
use repos::RawInfinitepushParams;
use repos::RawLfsParams;
use repos::RawLoggingDestination;
use repos::RawLoggingDestinationScribe;
use repos::RawMetadataCacheConfig;
use repos::RawMetadataCacheUpdateMode;
use repos::RawMetadataLoggerConfig;
use repos::RawModernSyncConfig;
use repos::RawPushParams;
use repos::RawPushrebaseParams;
use repos::RawPushrebaseRemoteMode;
use repos::RawPushrebaseRemoteModeRemote;
use repos::RawRemoteDerivationConfig;
use repos::RawRepoClientKnobs;
use repos::RawServiceWriteRestrictions;
use repos::RawShardedService;
use repos::RawShardingModeConfig;
use repos::RawSourceControlServiceMonitoring;
use repos::RawSourceControlServiceParams;
use repos::RawSparseProfilesConfig;
use repos::RawUpdateLoggingConfig;
use repos::RawUriGeneratorType;
use repos::RawWalkerConfig;
use repos::RawWalkerJobParams;
use repos::RawWalkerJobType;
use repos::RawXRepoSyncSourceConfig;
use repos::RawXRepoSyncSourceConfigMapping;
use repos::RawZelosConfig;

use crate::convert::Convert;
use crate::errors::ConfigurationError;

impl Convert for RawCacheWarmupConfig {
    type Output = CacheWarmupParams;

    fn convert(self) -> Result<Self::Output> {
        Ok(CacheWarmupParams {
            bookmark: BookmarkKey::new(self.bookmark)?,
            commit_limit: self
                .commit_limit
                .map(|v| v.try_into())
                .transpose()?
                .unwrap_or(200000),
            microwave_preload: self.microwave_preload.unwrap_or(false),
        })
    }
}

impl Convert for RawHookManagerParams {
    type Output = HookManagerParams;

    fn convert(self) -> Result<Self::Output> {
        Ok(HookManagerParams {
            disable_acl_checker: self.disable_acl_checker,
            all_hooks_bypassed: self.all_hooks_bypassed,
            bypassed_commits_scuba_table: self.bypassed_commits_scuba_table,
        })
    }
}

impl Convert for RawHookConfig {
    type Output = HookParams;

    fn convert(self) -> Result<Self::Output> {
        let bypass_commit_message = self.bypass_commit_string;

        let bypass_pushvar = self
            .bypass_pushvar
            .map(|s| {
                let parts: Vec<_> = s.split('=').collect();
                match parts.as_slice() {
                    [name, value] => Ok((name.to_string(), value.to_string())),
                    _ => Err(ConfigurationError::InvalidPushvar(s)),
                }
            })
            .transpose()?;

        let bypass = match (bypass_commit_message, bypass_pushvar) {
            (Some(msg), None) => Some(HookBypass::new_with_commit_msg(msg)),
            (None, Some((name, value))) => Some(HookBypass::new_with_pushvar(name, value)),
            (Some(msg), Some((name, value))) => Some(HookBypass::new_with_commit_msg_and_pushvar(
                msg, name, value,
            )),
            (None, None) => None,
        };

        let config = HookConfig {
            bypass,
            options: self.config_json,
            log_only: self.log_only.unwrap_or_default(),
            strings: self.config_strings.unwrap_or_default(),
            ints: self.config_ints.unwrap_or_default(),
            ints_64: self.config_ints_64.unwrap_or_default(),
            string_lists: self.config_string_lists.unwrap_or_default(),
            int_lists: self.config_int_lists.unwrap_or_default(),
            int_64_lists: self.config_int_64_lists.unwrap_or_default(),
        };

        Ok(HookParams {
            implementation: self.implementation.unwrap_or_else(|| self.name.clone()),
            name: self.name,
            config,
        })
    }
}

impl Convert for RawBookmarkConfig {
    type Output = BookmarkParams;

    fn convert(self) -> Result<Self::Output> {
        let bookmark_or_regex = match (self.regex, self.name) {
            (None, Some(name)) => BookmarkOrRegex::Bookmark(BookmarkKey::new(name).unwrap()),
            (Some(regex), None) => match Regex::new(&regex) {
                Ok(regex) => BookmarkOrRegex::Regex(ComparableRegex::new(regex)),
                Err(err) => {
                    return Err(ConfigurationError::InvalidConfig(format!(
                        "invalid bookmark regex: {}",
                        err
                    ))
                    .into());
                }
            },
            _ => {
                return Err(ConfigurationError::InvalidConfig(
                    "bookmark's params need to specify regex xor name".into(),
                )
                .into());
            }
        };

        let hooks = self.hooks.into_iter().map(|rbmh| rbmh.hook_name).collect();
        let only_fast_forward = self.only_fast_forward;
        let allowed_users = self
            .allowed_users
            .map(|re| Regex::new(&re))
            .transpose()?
            .map(ComparableRegex::new);
        let allowed_hipster_group = self.allowed_hipster_group;
        let rewrite_dates = self.rewrite_dates;
        let hooks_skip_ancestors_of = self
            .hooks_skip_ancestors_of
            .unwrap_or_default()
            .into_iter()
            .map(BookmarkKey::new)
            .collect::<Result<Vec<_>, _>>()?;
        let ensure_ancestor_of = self.ensure_ancestor_of.map(BookmarkKey::new).transpose()?;
        let allow_move_to_public_commits_without_hooks = self
            .allow_move_to_public_commits_without_hooks
            .unwrap_or(false);

        Ok(BookmarkParams {
            bookmark: bookmark_or_regex,
            hooks,
            only_fast_forward,
            allowed_users,
            allowed_hipster_group,
            rewrite_dates,
            hooks_skip_ancestors_of,
            ensure_ancestor_of,
            allow_move_to_public_commits_without_hooks,
        })
    }
}

impl Convert for RawPushParams {
    type Output = PushParams;

    fn convert(self) -> Result<Self::Output> {
        let default = PushParams::default();
        Ok(PushParams {
            pure_push_allowed: self.pure_push_allowed.unwrap_or(default.pure_push_allowed),
            unbundle_commit_limit: self
                .unbundle_commit_limit
                .map(|limit| limit.try_into())
                .transpose()?,
        })
    }
}

impl Convert for RawCommitIdentityScheme {
    type Output = CommitIdentityScheme;

    fn convert(self) -> Result<Self::Output> {
        let converted = match self {
            RawCommitIdentityScheme::HG => CommitIdentityScheme::HG,
            RawCommitIdentityScheme::GIT => CommitIdentityScheme::GIT,
            RawCommitIdentityScheme::BONSAI => CommitIdentityScheme::BONSAI,
            RawCommitIdentityScheme::UNKNOWN => CommitIdentityScheme::UNKNOWN,
            v => return Err(anyhow!("Invalid value {} for enum CommitIdentityScheme", v)),
        };
        Ok(converted)
    }
}

impl Convert for RawPushrebaseRemoteModeRemote {
    type Output = Address;

    fn convert(self) -> Result<Self::Output> {
        match self {
            Self::tier(t) => Ok(Address::Tier(t)),
            Self::host_port(host) => Ok(Address::HostPort(host)),
            Self::UnknownField(e) => anyhow::bail!("Unknown field: {}", e),
        }
    }
}

impl Convert for RawPushrebaseRemoteMode {
    type Output = PushrebaseRemoteMode;

    fn convert(self) -> Result<Self::Output> {
        match self {
            Self::local(_) => Ok(PushrebaseRemoteMode::Local),
            Self::remote_land_service(addr) => {
                Ok(PushrebaseRemoteMode::RemoteLandService(addr.convert()?))
            }
            Self::remote_land_service_local_fallback(addr) => Ok(
                PushrebaseRemoteMode::RemoteLandServiceWithLocalFallback(addr.convert()?),
            ),
            Self::UnknownField(e) => anyhow::bail!("Unknown field: {}", e),
        }
    }
}

impl Convert for RawPushrebaseParams {
    type Output = PushrebaseParams;

    fn convert(self) -> Result<Self::Output> {
        let default = PushrebaseParams::default();
        Ok(PushrebaseParams {
            flags: PushrebaseFlags {
                rewritedates: self.rewritedates.unwrap_or(default.flags.rewritedates),
                recursion_limit: self
                    .recursion_limit
                    .map(|v| v.try_into())
                    .transpose()?
                    .or(default.flags.recursion_limit),
                forbid_p2_root_rebases: self
                    .forbid_p2_root_rebases
                    .unwrap_or(default.flags.forbid_p2_root_rebases),
                casefolding_check: self
                    .casefolding_check
                    .unwrap_or(default.flags.casefolding_check),
                casefolding_check_excluded_paths: self
                    .casefolding_check_excluded_paths
                    .map(|raw| {
                        raw.into_iter()
                            .map(|path| NonRootMPath::new_opt(path.as_bytes()).map(MPath::from))
                            .collect::<Result<PrefixTrie>>()
                    })
                    .transpose()?
                    .unwrap_or_default(),
                not_generated_filenodes_limit: 500,
                monitoring_bookmark: self.monitoring_bookmark,
            },
            block_merges: self.block_merges.unwrap_or(default.block_merges),
            emit_obsmarkers: self.emit_obsmarkers.unwrap_or(default.emit_obsmarkers),
            globalrev_config: self
                .globalrevs_publishing_bookmark
                .map(|bookmark| {
                    anyhow::Ok(GlobalrevConfig {
                        publishing_bookmark: BookmarkKey::new(bookmark)?,
                        globalrevs_small_repo_id: self
                            .globalrevs_small_repo_id
                            .map(RepositoryId::new),
                    })
                })
                .transpose()?,
            populate_git_mapping: self
                .populate_git_mapping
                .unwrap_or(default.populate_git_mapping),
            allow_change_xrepo_mapping_extra: self
                .allow_change_xrepo_mapping_extra
                .unwrap_or(false),
            remote_mode: self
                .remote_mode
                .map_or(Ok(default.remote_mode), Convert::convert)?,
        })
    }
}

impl Convert for RawLfsParams {
    type Output = LfsParams;

    fn convert(self) -> Result<Self::Output> {
        Ok(LfsParams {
            threshold: self.threshold.map(|v| v.try_into()).transpose()?,
            rollout_percentage: self.rollout_percentage.unwrap_or(0).try_into()?,
            use_upstream_lfs_server: self.use_upstream_lfs_server.unwrap_or(false),
        })
    }
}

impl Convert for RawInfinitepushParams {
    type Output = InfinitepushParams;

    fn convert(self) -> Result<Self::Output> {
        Ok(InfinitepushParams {
            allow_writes: self.allow_writes,
            namespace: self
                .namespace_pattern
                .and_then(|ns| Regex::new(&ns).ok().map(InfinitepushNamespace::new)),
            hydrate_getbundle_response: self.hydrate_getbundle_response.unwrap_or(false),
        })
    }
}

impl Convert for RawSourceControlServiceParams {
    type Output = SourceControlServiceParams;

    fn convert(self) -> Result<Self::Output> {
        let service_write_restrictions = self
            .service_write_restrictions
            .unwrap_or_default()
            .into_iter()
            .map(|(name, raw)| Ok((name, raw.convert()?)))
            .collect::<Result<HashMap<_, _>>>()?;

        Ok(SourceControlServiceParams {
            permit_writes: self.permit_writes,
            permit_service_writes: self.permit_service_writes,
            service_write_hipster_acl: self.service_write_hipster_acl,
            permit_commits_without_parents: self.permit_commits_without_parents,
            service_write_restrictions,
        })
    }
}

impl Convert for RawServiceWriteRestrictions {
    type Output = ServiceWriteRestrictions;

    fn convert(self) -> Result<Self::Output> {
        let RawServiceWriteRestrictions {
            permitted_methods,
            permitted_path_prefixes,
            permitted_bookmarks,
            permitted_bookmark_regex,
            ..
        } = self;

        let permitted_methods = permitted_methods.into_iter().collect();

        let permitted_path_prefixes = permitted_path_prefixes
            .map(|raw| {
                raw.into_iter()
                    .map(|path| {
                        NonRootMPath::new_opt(path.as_bytes())
                            .map(mononoke_types::path::MPath::from)
                    })
                    .collect::<Result<PrefixTrie>>()
            })
            .transpose()?
            .unwrap_or_default();

        let permitted_bookmarks = permitted_bookmarks
            .unwrap_or_default()
            .into_iter()
            .collect();

        let permitted_bookmark_regex = permitted_bookmark_regex
            .as_deref()
            .map(Regex::new)
            .transpose()
            .context("invalid service write permitted bookmark regex")?
            .map(ComparableRegex::new);

        Ok(ServiceWriteRestrictions {
            permitted_methods,
            permitted_path_prefixes,
            permitted_bookmarks,
            permitted_bookmark_regex,
        })
    }
}

impl Convert for RawSourceControlServiceMonitoring {
    type Output = SourceControlServiceMonitoring;

    fn convert(self) -> Result<Self::Output> {
        let bookmarks_to_report_age = self
            .bookmarks_to_report_age
            .into_iter()
            .map(BookmarkKey::new)
            .collect::<Result<Vec<_>>>()?;
        Ok(SourceControlServiceMonitoring {
            bookmarks_to_report_age,
        })
    }
}

impl Convert for RawDerivedDataTypesConfig {
    type Output = DerivedDataTypesConfig;

    fn convert(self) -> Result<Self::Output> {
        let types = self
            .types
            .into_iter()
            .map(|ty| DerivableType::from_name(&ty))
            .collect::<Result<_>>()?;
        let ephemeral_bubbles_disabled_types = self
            .ephemeral_bubbles_disabled_types
            .unwrap_or_default()
            .into_iter()
            .map(|ty| DerivableType::from_name(&ty))
            .collect::<Result<_>>()?;
        let mapping_key_prefixes = self
            .mapping_key_prefixes
            .into_iter()
            .map(|(k, _v)| Ok((DerivableType::from_name(&k)?, _v)))
            .collect::<Result<_>>()?;
        let unode_version = match self.unode_version {
            None => UnodeVersion::default(),
            Some(1) => return Err(anyhow!("unode version 1 has been deprecated")),
            Some(2) => UnodeVersion::V2,
            Some(version) => return Err(anyhow!("unknown unode version {}", version)),
        };
        let blame_filesize_limit = self.blame_filesize_limit.map(|limit| limit as u64);
        let blame_version = match self.blame_version {
            None => BlameVersion::default(),
            Some(1) => return Err(anyhow!("blame version 1 has been deprecated")),
            Some(2) => BlameVersion::V2,
            Some(version) => return Err(anyhow!("unknown blame version {}", version)),
        };
        let git_delta_manifest_version = match self.git_delta_manifest_version {
            None => GitDeltaManifestVersion::default(),
            Some(2) => GitDeltaManifestVersion::V2,
            Some(3) => GitDeltaManifestVersion::V3,
            Some(version) => return Err(anyhow!("unknown git delta manifest version {}", version)),
        };
        let git_delta_manifest_v2_config = self
            .git_delta_manifest_v2_config
            .map(|raw| raw.convert())
            .transpose()?;
        let git_delta_manifest_v3_config = self
            .git_delta_manifest_v3_config
            .map(|raw| raw.convert())
            .transpose()?;

        let derivation_batch_sizes = self
            .derivation_batch_sizes
            .unwrap_or_default()
            .into_iter()
            .map(|(k, v)| Ok((DerivableType::from_name(&k)?, v.try_into()?)))
            .collect::<Result<_>>()?;

        let inferred_copy_from_config = self
            .inferred_copy_from_config
            .map(|raw| raw.convert())
            .transpose()?;

        Ok(DerivedDataTypesConfig {
            types,
            ephemeral_bubbles_disabled_types,
            mapping_key_prefixes,
            unode_version,
            blame_filesize_limit,
            hg_set_committer_extra: self.hg_set_committer_extra.unwrap_or(false),
            blame_version,
            git_delta_manifest_version,
            git_delta_manifest_v2_config,
            git_delta_manifest_v3_config,
            derivation_batch_sizes,
            inferred_copy_from_config,
        })
    }
}

impl Convert for RawInferredCopyFromConfig {
    type Output = InferredCopyFromConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(InferredCopyFromConfig {
            dir_level_for_basename_lookup: self.dir_level_for_basename_lookup as usize,
        })
    }
}

impl Convert for RawGitDeltaManifestV2Config {
    type Output = GitDeltaManifestV2Config;

    fn convert(self) -> Result<Self::Output> {
        Ok(GitDeltaManifestV2Config {
            max_inlined_object_size: self.max_inlined_object_size as usize,
            max_inlined_delta_size: self.max_inlined_delta_size as u64,
            delta_chunk_size: self.delta_chunk_size as u64,
        })
    }
}

impl Convert for RawGitDeltaManifestV3Config {
    type Output = GitDeltaManifestV3Config;

    fn convert(self) -> Result<Self::Output> {
        Ok(GitDeltaManifestV3Config {
            max_inlined_object_size: self.max_inlined_object_size as usize,
            max_inlined_delta_size: self.max_inlined_delta_size as u64,
            delta_chunk_size: self.delta_chunk_size as u64,
            entry_chunk_size: self.entry_chunk_size as usize,
        })
    }
}

impl Convert for RawDerivedDataConfig {
    type Output = DerivedDataConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(DerivedDataConfig {
            scuba_table: self.scuba_table,
            enabled_config_name: self.enabled_config_name.unwrap_or_default(),
            available_configs: self
                .available_configs
                .unwrap_or_default()
                .into_iter()
                .map(|(s, raw_config)| Ok((s, raw_config.convert()?)))
                .collect::<Result<_, anyhow::Error>>()?,
            derivation_queue_scuba_table: self.derivation_queue_scuba_table,
            remote_derivation_config: self
                .remote_derivation_config
                .map(|raw| raw.convert())
                .transpose()?,
            blocked_derivation: self
                .blocked_derivation
                .map(|blocked_derivation| blocked_derivation.convert())
                .transpose()?
                .unwrap_or_default(),
        })
    }
}

impl Convert for RawDerivedDataBlockedDerivation {
    type Output = HashMap<ChangesetId, Option<HashSet<DerivableType>>>;

    fn convert(self) -> Result<Self::Output> {
        self.changesets
            .into_iter()
            .map(|(csid, blocked_derivation)| {
                Ok((ChangesetId::from_str(&csid)?, blocked_derivation.convert()?))
            })
            .collect()
    }
}

impl Convert for RawDerivedDataBlockedChangesetDerivation {
    type Output = Option<HashSet<DerivableType>>;

    fn convert(self) -> Result<Self::Output> {
        self.blocked_derived_data_types
            .map(|types| {
                types
                    .into_iter()
                    .map(|ty| DerivableType::from_name(&ty))
                    .collect::<Result<HashSet<_>, _>>()
            })
            .transpose()
    }
}

impl Convert for RawRemoteDerivationConfig {
    type Output = RemoteDerivationConfig;

    fn convert(self) -> Result<Self::Output> {
        match self {
            RawRemoteDerivationConfig::shard_manager_tier(shard_manager_tier) => {
                Ok(RemoteDerivationConfig::ShardManagerTier(shard_manager_tier))
            }
            RawRemoteDerivationConfig::smc_tier(smc_tier) => {
                Ok(RemoteDerivationConfig::SmcTier(smc_tier))
            }
            RawRemoteDerivationConfig::host_port(host_port) => {
                Ok(RemoteDerivationConfig::HostPort(host_port))
            }
            RawRemoteDerivationConfig::UnknownField(e) => {
                anyhow::bail!("Unknown variant of RawRemoteDerivationConfig: {}", e)
            }
        }
    }
}

impl Convert for RawRepoClientKnobs {
    type Output = RepoClientKnobs;

    fn convert(self) -> Result<Self::Output> {
        Ok(RepoClientKnobs {
            allow_short_getpack_history: self.allow_short_getpack_history,
        })
    }
}

impl Convert for RawWalkerJobType {
    type Output = WalkerJobType;

    fn convert(self) -> Result<Self::Output> {
        let job_type = match self {
            RawWalkerJobType::SCRUB_ALL_CHUNKED => WalkerJobType::ScrubAllChunked,
            RawWalkerJobType::SCRUB_DERIVED_CHUNKED => WalkerJobType::ScrubDerivedChunked,
            RawWalkerJobType::SCRUB_DERIVED_NO_CONTENT_META => {
                WalkerJobType::ScrubDerivedNoContentMeta
            }
            RawWalkerJobType::SCRUB_DERIVED_NO_CONTENT_META_CHUNKED => {
                WalkerJobType::ScrubDerivedNoContentMetaChunked
            }
            RawWalkerJobType::SCRUB_HG_ALL_CHUNKED => WalkerJobType::ScrubHgAllChunked,
            RawWalkerJobType::SCRUB_HG_FILE_CONTENT => WalkerJobType::ScrubHgFileContent,
            RawWalkerJobType::SCRUB_HG_FILE_NODE => WalkerJobType::ScrubHgFileNode,
            RawWalkerJobType::SCRUB_UNODE_ALL_CHUNKED => WalkerJobType::ScrubUnodeAllChunked,
            RawWalkerJobType::SCRUB_UNODE_BLAME => WalkerJobType::ScrubUnodeBlame,
            RawWalkerJobType::SCRUB_UNODE_FASTLOG => WalkerJobType::ScrubUnodeFastlog,
            RawWalkerJobType::SHALLOW_HG_SCRUB => WalkerJobType::ShallowHgScrub,
            RawWalkerJobType::VALIDATE_ALL => WalkerJobType::ValidateAll,
            RawWalkerJobType::UNKNOWN => WalkerJobType::Unknown,
            v => return Err(anyhow!("Invalid value {} for enum WalkerJobType", v)),
        };
        Ok(job_type)
    }
}

impl Convert for RawWalkerJobParams {
    type Output = WalkerJobParams;

    fn convert(self) -> Result<Self::Output> {
        Ok(WalkerJobParams {
            scheduled_max_concurrency: self.scheduled_max_concurrency,
            qps_limit: self.qps_limit,
            exclude_node_type: self.exclude_node_type,
            allow_remaining_deferred: self.allow_remaining_deferred.is_some_and(|v| v),
            error_as_node_data_type: self.error_as_node_data_type,
        })
    }
}

impl Convert for RawWalkerConfig {
    type Output = WalkerConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(WalkerConfig {
            scrub_enabled: self.scrub_enabled,
            validate_enabled: self.validate_enabled,
            params: self
                .params
                .map(|p| {
                    p.into_iter()
                        .map(|(k, v)| anyhow::Ok((k.convert()?, v.convert()?)))
                        .collect::<Result<_, _>>()
                })
                .transpose()?,
        })
    }
}

impl Convert for RawCrossRepoCommitValidationConfig {
    type Output = CrossRepoCommitValidation;

    fn convert(self) -> Result<Self::Output> {
        let skip_bookmarks = self
            .skip_bookmarks
            .into_iter()
            .map(BookmarkKey::new)
            .collect::<Result<_, _>>()?;
        Ok(CrossRepoCommitValidation { skip_bookmarks })
    }
}

impl Convert for RawSparseProfilesConfig {
    type Output = SparseProfilesConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(SparseProfilesConfig {
            sparse_profiles_location: self.sparse_profiles_location,
            excluded_paths: self.excluded_paths.unwrap_or_default(),
            monitored_profiles: self.monitored_profiles.unwrap_or_default(),
        })
    }
}

impl Convert for RawCasSyncConfig {
    type Output = MononokeCasSyncConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(MononokeCasSyncConfig {
            main_bookmark_to_sync: self.main_bookmark_to_sync,
            sync_all_bookmarks: self.sync_all_bookmarks,
            use_case_public: self.use_case_public,
            use_case_draft: self.use_case_draft,
        })
    }
}

impl Convert for RawModernSyncConfig {
    type Output = ModernSyncConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(ModernSyncConfig {
            url: self.url,
            chunk_size: self.chunk_size,
            single_db_query_entries_limit: self.single_db_query_entries_limit,
            changeset_concurrency: self.changeset_concurrency,
            max_blob_bytes: self.max_blob_bytes,
            content_channel_config: self.content_channel_config.convert()?,
            filenodes_channel_config: self.filenodes_channel_config.convert()?,
            trees_channel_config: self.trees_channel_config.convert()?,
            changesets_channel_config: self.changesets_channel_config.convert()?,
        })
    }
}

impl Convert for RawModernSyncChannelConfig {
    type Output = ModernSyncChannelConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(ModernSyncChannelConfig {
            batch_size: self.batch_size,
            channel_size: self.channel_size,
            flush_interval_ms: self.flush_interval_ms,
        })
    }
}

impl Convert for RawLoggingDestination {
    type Output = LoggingDestination;

    fn convert(self) -> Result<Self::Output> {
        let dest = match self {
            Self::logger(_) => LoggingDestination::Logger,
            Self::scribe(RawLoggingDestinationScribe { scribe_category }) => {
                LoggingDestination::Scribe { scribe_category }
            }
            Self::UnknownField(f) => {
                return Err(anyhow!("Unknown variant {} of RawLoggingDestination", f));
            }
        };
        Ok(dest)
    }
}

impl Convert for RawUpdateLoggingConfig {
    type Output = UpdateLoggingConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(UpdateLoggingConfig {
            bookmark_logging_destination: self.bookmark_logging_destination.convert()?,
            new_commit_logging_destination: self.new_commit_logging_destination.convert()?,
            git_content_refs_logging_destination: self
                .git_content_refs_logging_destination
                .convert()?,
        })
    }
}

impl Convert for RawCommitGraphConfig {
    type Output = CommitGraphConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(CommitGraphConfig {
            scuba_table: self.scuba_table,
            preloaded_commit_graph_blobstore_key: self.preloaded_commit_graph_blobstore_key,
            disable_commit_graph_v2_with_empty_common: self
                .disable_commit_graph_v2_with_empty_common,
        })
    }
}

impl Convert for RawMetadataLoggerConfig {
    type Output = MetadataLoggerConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(MetadataLoggerConfig {
            bookmarks: self
                .bookmarks
                .into_iter()
                .map(BookmarkKey::new)
                .collect::<Result<_>>()?,
            sleep_interval_secs: self.sleep_interval_secs.try_into()?,
        })
    }
}

impl Convert for RawZelosConfig {
    type Output = ZelosConfig;

    fn convert(self) -> Result<Self::Output> {
        match self {
            Self::local_zelos_port(port) => Ok(ZelosConfig::Local {
                port: port.try_into()?,
            }),
            Self::zelos_tier(tier) => Ok(ZelosConfig::Remote { tier }),
            Self::UnknownField(f) => Err(anyhow!("Unknown variant {} of RawZelosConfig", f)),
        }
    }
}

impl Convert for RawGitBundleURIConfig {
    type Output = GitBundleURIConfig;

    fn convert(self) -> Result<Self::Output> {
        match self.uri_generator_type {
            RawUriGeneratorType::cdn(cdn) => Ok(GitBundleURIConfig {
                uri_generator_type: UriGeneratorType::Cdn {
                    bucket: cdn.bucket,
                    api_key: cdn.api_key,
                },
                trusted_only: self.trusted_only,
            }),
            RawUriGeneratorType::manifold(manifold) => Ok(GitBundleURIConfig {
                uri_generator_type: UriGeneratorType::Manifold {
                    bucket: manifold.bucket,
                    api_key: manifold.api_key,
                },
                trusted_only: self.trusted_only,
            }),
            RawUriGeneratorType::local_fs(_) => Ok(GitBundleURIConfig {
                uri_generator_type: UriGeneratorType::LocalFS,
                trusted_only: self.trusted_only,
            }),
            RawUriGeneratorType::UnknownField(f) => {
                Err(anyhow!("Unknown variant {} of RawGitBundleURIConfig", f))
            }
        }
    }
}

impl Convert for RawShardedService {
    type Output = ShardedService;

    fn convert(self) -> Result<Self::Output> {
        let service = match self {
            RawShardedService::EDEN_API => ShardedService::SaplingRemoteApi,
            RawShardedService::SOURCE_CONTROL_SERVICE => ShardedService::SourceControlService,
            RawShardedService::DERIVED_DATA_SERVICE => ShardedService::DerivedDataService,
            RawShardedService::LAND_SERVICE => ShardedService::LandService,
            RawShardedService::LARGE_FILES_SERVICE => ShardedService::LargeFilesService,
            RawShardedService::DERIVATION_WORKER => ShardedService::DerivationWorker,
            RawShardedService::ASYNC_REQUESTS_WORKER => ShardedService::AsyncRequestsWorker,
            RawShardedService::WALKER_SCRUB_ALL => ShardedService::WalkerScrubAll,
            RawShardedService::WALKER_VALIDATE_ALL => ShardedService::WalkerValidateAll,
            RawShardedService::DERIVED_DATA_TAILER => ShardedService::DerivedDataTailer,
            RawShardedService::ALIAS_VERIFY => ShardedService::AliasVerify,
            RawShardedService::DRAFT_COMMIT_DELETION => ShardedService::DraftCommitDeletion,
            RawShardedService::MONONOKE_GIT_SERVER => ShardedService::MononokeGitServer,
            RawShardedService::REPO_METADATA_LOGGER => ShardedService::RepoMetadataLogger,
            RawShardedService::BOOKMARK_SERVICE => ShardedService::BookmarkService,
            v => return Err(anyhow!("Invalid value {} for enum ShardedService", v)),
        };
        Ok(service)
    }
}

impl Convert for RawShardingModeConfig {
    type Output = ShardingModeConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(ShardingModeConfig {
            status: self
                .status
                .into_iter()
                // Since this is a simple type conversion, the only error that can be encountered would be due to an
                // unknown enum value. If that happens, it means we have a config that has more values than the code understands. In
                // such a case, it should be safe to ignore this unknown value cause the existing code can work without it.
                .filter_map(|(k, v)| k.convert().map(|k| (k, v)).ok())
                .collect(),
        })
    }
}

impl Convert for RawGitConcurrencyParams {
    type Output = GitConcurrencyParams;

    fn convert(self) -> Result<Self::Output> {
        Ok(GitConcurrencyParams {
            trees_and_blobs: self.trees_and_blobs.try_into()?,
            commits: self.commits.try_into()?,
            tags: self.tags.try_into()?,
            shallow: self.shallow.try_into()?,
        })
    }
}

impl Convert for RawGitConfigs {
    type Output = GitConfigs;

    fn convert(self) -> Result<Self::Output> {
        let git_concurrency = self.git_concurrency.convert()?;
        let git_lfs_interpret_pointers = self.git_lfs_interpret_pointers.unwrap_or(false);

        let fetch_message = self.fetch_message;

        let git_bundle_uri = self.git_bundle_uri_config.convert()?;

        Ok(GitConfigs {
            git_concurrency,
            git_lfs_interpret_pointers,
            fetch_message,
            git_bundle_uri,
        })
    }
}

impl Convert for RawXRepoSyncSourceConfig {
    type Output = XRepoSyncSourceConfig;

    fn convert(self) -> Result<Self::Output> {
        Ok(XRepoSyncSourceConfig {
            bookmark_regex: self.bookmark_regex,
            backsync_enabled: self.backsync_enabled,
        })
    }
}

impl Convert for RawXRepoSyncSourceConfigMapping {
    type Output = XRepoSyncSourceConfigMapping;

    fn convert(self) -> Result<Self::Output> {
        let mapping = self
            .mapping
            .into_iter()
            .map(|(repo_name, x_repo_sync_source_config)| {
                Ok((repo_name, x_repo_sync_source_config.convert()?))
            })
            .collect::<Result<_>>()?;
        Ok(XRepoSyncSourceConfigMapping { mapping })
    }
}

impl Convert for RawCommitCloudConfig {
    type Output = CommitCloudConfig;
    fn convert(self) -> Result<Self::Output> {
        Ok(CommitCloudConfig {
            mocked_employees: self.mocked_employees,
            disable_interngraph_notification: self.disable_interngraph_notification,
        })
    }
}

impl Convert for RawMetadataCacheUpdateMode {
    type Output = MetadataCacheUpdateMode;
    fn convert(self) -> Result<Self::Output> {
        let cache_update_mode = match self {
            RawMetadataCacheUpdateMode::tailing(tailing) => MetadataCacheUpdateMode::Tailing {
                category: tailing.category,
            },
            RawMetadataCacheUpdateMode::polling(_) => MetadataCacheUpdateMode::Polling,
            RawMetadataCacheUpdateMode::UnknownField(f) => {
                bail!("Unsupported MetadataCacheUpdateMode {}", f)
            }
        };
        Ok(cache_update_mode)
    }
}

impl Convert for RawMetadataCacheConfig {
    type Output = MetadataCacheConfig;
    fn convert(self) -> Result<Self::Output> {
        Ok(MetadataCacheConfig {
            wbc_update_mode: self
                .wbc_update_mode
                .map(|mode| mode.convert())
                .transpose()?,
            tags_update_mode: self
                .tags_update_mode
                .map(|mode| mode.convert())
                .transpose()?,
            content_refs_update_mode: self
                .content_refs_update_mode
                .map(|mode| mode.convert())
                .transpose()?,
        })
    }
}
