/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::TryInto;

use anyhow::{anyhow, Result};
use bookmarks_types::BookmarkName;
use metaconfig_types::{
    BookmarkOrRegex, BookmarkParams, Bundle2ReplayParams, CacheWarmupParams, DerivedDataConfig,
    HookBypass, HookConfig, HookManagerParams, HookParams, InfinitepushNamespace,
    InfinitepushParams, LfsParams, PushParams, PushrebaseFlags, PushrebaseParams,
    SourceControlServiceMonitoring, SourceControlServiceParams, StorageConfig, UnodeVersion,
    WireprotoLoggingConfig,
};
use regex::Regex;
use repos::{
    RawBookmarkConfig, RawBundle2ReplayParams, RawCacheWarmupConfig, RawDerivedDataConfig,
    RawHookConfig, RawHookManagerParams, RawInfinitepushParams, RawLfsParams, RawPushParams,
    RawPushrebaseParams, RawSourceControlServiceMonitoring, RawSourceControlServiceParams,
    RawUnodeVersion, RawWireprotoLoggingConfig,
};

use crate::convert::Convert;
use crate::errors::ConfigurationError;

pub(crate) const DEFAULT_ARG_SIZE_THRESHOLD: u64 = 500_000;

pub(crate) fn convert_wireproto_logging_config(
    raw: RawWireprotoLoggingConfig,
    get_storage: impl Fn(&str) -> Result<StorageConfig>,
) -> Result<WireprotoLoggingConfig> {
    let RawWireprotoLoggingConfig {
        scribe_category,
        storage_config: wireproto_storage_config,
        remote_arg_size_threshold,
        local_path,
    } = raw;

    let storage_config_and_threshold = match (wireproto_storage_config, remote_arg_size_threshold) {
        (Some(storage_config), Some(threshold)) => Some((storage_config, threshold as u64)),
        (None, Some(_threshold)) => {
            return Err(
                anyhow!("Invalid configuration: wireproto threshold is specified, but storage config is not")
            );
        }
        (Some(storage_config), None) => Some((storage_config, DEFAULT_ARG_SIZE_THRESHOLD)),
        (None, None) => None,
    };

    let storage_config_and_threshold = storage_config_and_threshold
        .map(|(storage_config, threshold)| {
            get_storage(&storage_config).map(|config| (config, threshold))
        })
        .transpose()?;

    Ok(WireprotoLoggingConfig {
        scribe_category,
        storage_config_and_threshold,
        local_path,
    })
}

impl Convert for RawCacheWarmupConfig {
    type Output = CacheWarmupParams;

    fn convert(self) -> Result<Self::Output> {
        Ok(CacheWarmupParams {
            bookmark: BookmarkName::new(self.bookmark)?,
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
        })
    }
}

impl Convert for RawHookConfig {
    type Output = HookParams;

    fn convert(self) -> Result<Self::Output> {
        let bypass_commit_message = self.bypass_commit_string.map(HookBypass::CommitMessage);

        let bypass_pushvar = self
            .bypass_pushvar
            .map(|s| {
                let parts: Vec<_> = s.split('=').collect();
                match parts.as_slice() {
                    [name, value] => Ok(HookBypass::Pushvar {
                        name: name.to_string(),
                        value: value.to_string(),
                    }),
                    _ => Err(ConfigurationError::InvalidPushvar(s)),
                }
            })
            .transpose()?;

        if bypass_commit_message.is_some() && bypass_pushvar.is_some() {
            return Err(ConfigurationError::TooManyBypassOptions(self.name).into());
        }
        let bypass = bypass_commit_message.or(bypass_pushvar);

        let config = HookConfig {
            bypass,
            strings: self.config_strings.unwrap_or_default(),
            ints: self.config_ints.unwrap_or_default(),
        };

        Ok(HookParams {
            name: self.name,
            config,
        })
    }
}

impl Convert for RawBookmarkConfig {
    type Output = BookmarkParams;

    fn convert(self) -> Result<Self::Output> {
        let bookmark_or_regex = match (self.regex, self.name) {
            (None, Some(name)) => BookmarkOrRegex::Bookmark(BookmarkName::new(name).unwrap()),
            (Some(regex), None) => match Regex::new(&regex) {
                Ok(regex) => BookmarkOrRegex::Regex(regex),
                Err(err) => {
                    return Err(ConfigurationError::InvalidConfig(format!(
                        "invalid bookmark regex: {}",
                        err
                    ))
                    .into())
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
        let allowed_users = self.allowed_users.map(|re| Regex::new(&re)).transpose()?;
        let rewrite_dates = self.rewrite_dates;

        Ok(BookmarkParams {
            bookmark: bookmark_or_regex,
            hooks,
            only_fast_forward,
            allowed_users,
            rewrite_dates,
        })
    }
}

impl Convert for RawPushParams {
    type Output = PushParams;

    fn convert(self) -> Result<Self::Output> {
        let default = PushParams::default();
        Ok(PushParams {
            pure_push_allowed: self.pure_push_allowed.unwrap_or(default.pure_push_allowed),
        })
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
                not_generated_filenodes_limit: 500,
            },
            commit_scribe_category: self.commit_scribe_category,
            block_merges: self.block_merges.unwrap_or(default.block_merges),
            emit_obsmarkers: self.emit_obsmarkers.unwrap_or(default.emit_obsmarkers),
            assign_globalrevs: self.assign_globalrevs.unwrap_or(default.assign_globalrevs),
            populate_git_mapping: self
                .populate_git_mapping
                .unwrap_or(default.populate_git_mapping),
        })
    }
}

impl Convert for RawBundle2ReplayParams {
    type Output = Bundle2ReplayParams;

    fn convert(self) -> Result<Self::Output> {
        Ok(Bundle2ReplayParams {
            preserve_raw_bundle2: self.preserve_raw_bundle2.unwrap_or(false),
        })
    }
}

impl Convert for RawLfsParams {
    type Output = LfsParams;

    fn convert(self) -> Result<Self::Output> {
        Ok(LfsParams {
            threshold: self.threshold.map(|v| v.try_into()).transpose()?,
            rollout_percentage: self.rollout_percentage.unwrap_or(0).try_into()?,
            generate_lfs_blob_in_hg_sync_job: self
                .generate_lfs_blob_in_hg_sync_job
                .unwrap_or(false),
            rollout_smc_tier: self.rollout_smc_tier,
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
            populate_reverse_filler_queue: self.populate_reverse_filler_queue.unwrap_or(false),
        })
    }
}

impl Convert for RawSourceControlServiceParams {
    type Output = SourceControlServiceParams;

    fn convert(self) -> Result<Self::Output> {
        Ok(SourceControlServiceParams {
            permit_writes: self.permit_writes,
        })
    }
}

impl Convert for RawSourceControlServiceMonitoring {
    type Output = SourceControlServiceMonitoring;

    fn convert(self) -> Result<Self::Output> {
        let bookmarks_to_report_age = self
            .bookmarks_to_report_age
            .into_iter()
            .map(BookmarkName::new)
            .collect::<Result<Vec<_>>>()?;
        Ok(SourceControlServiceMonitoring {
            bookmarks_to_report_age,
        })
    }
}

impl Convert for RawDerivedDataConfig {
    type Output = DerivedDataConfig;

    fn convert(self) -> Result<Self::Output> {
        let unode_version = if let Some(unode_version) = self.raw_unode_version {
            match unode_version {
                RawUnodeVersion::unode_version_v1(_) => UnodeVersion::V1,
                RawUnodeVersion::unode_version_v2(_) => UnodeVersion::V2,
                RawUnodeVersion::UnknownField(field) => {
                    return Err(anyhow!("unknown unode version {}", field));
                }
            }
        } else {
            UnodeVersion::default()
        };

        Ok(DerivedDataConfig {
            scuba_table: self.scuba_table,
            derived_data_types: self.derived_data_types.unwrap_or_default(),
            unode_version,
        })
    }
}
