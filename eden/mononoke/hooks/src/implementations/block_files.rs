/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::HookConfig;
use mononoke_types::BasicFileChange;
use mononoke_types::NonRootMPath;
use regex::Regex;
use serde::Deserialize;

use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::HookFileContentProvider;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;

#[derive(Deserialize, Clone, Debug)]
pub struct BlockFilesConfig {
    /// Deny patterns for all sources of pushes
    #[serde(with = "serde_regex", default)]
    block_patterns: Vec<Regex>,
    /// Deny patterns for pushes, originally intended to this repo
    /// (as opposed to push-redirected ones)
    #[serde(with = "serde_regex", default)]
    native_push_only_block_patterns: Vec<Regex>,
}

pub struct BlockFilesHook {
    config: BlockFilesConfig,
}

impl BlockFilesHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(config.parse_options()?)
    }

    pub fn with_config(config: BlockFilesConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl FileHook for BlockFilesHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _content_manager: &'fetcher dyn HookFileContentProvider,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }

        block_unacceptable_patterns(&self.config, path, cross_repo_push_source, change)
    }
}

fn rejection<'a, 'b>(path: &'a String, pattern: &'b Regex) -> HookExecution {
    HookExecution::Rejected(HookRejectionInfo::new_long(
        "Blocked filename matched name pattern",
        format!(
            "Blocked filename '{}' matched name pattern '{}'. Rename or remove this file and try again.",
            path, pattern
        ),
    ))
}

/// Easily-testable business logic of the `BlockFiles` hook
fn block_unacceptable_patterns<'a, 'b>(
    config: &BlockFilesConfig,
    path: &'a NonRootMPath,
    cross_repo_push_source: CrossRepoPushSource,
    change: Option<&'b BasicFileChange>,
) -> Result<HookExecution> {
    let path = path.to_string();
    if CrossRepoPushSource::NativeToThisRepo == cross_repo_push_source {
        for pattern in &config.native_push_only_block_patterns {
            if pattern.is_match(&path) {
                return Ok(rejection(&path, pattern));
            }
        }
    }
    if change.is_none() {
        // It is acceptable to delete any file
        return Ok(HookExecution::Accepted);
    }

    for pattern in &config.block_patterns {
        if pattern.is_match(&path) {
            return Ok(rejection(&path, pattern));
        }
    }

    Ok(HookExecution::Accepted)
}

#[cfg(test)]
mod test {
    use mononoke_types::FileType;
    use mononoke_types_mocks::contentid::TWOS_CTID;

    use super::*;

    fn setup_config() -> BlockFilesConfig {
        let all = vec!["all".try_into().unwrap()];
        let native = vec!["native".try_into().unwrap()];
        BlockFilesConfig {
            block_patterns: all,
            native_push_only_block_patterns: native,
        }
    }

    fn basic_change() -> BasicFileChange {
        BasicFileChange::new(TWOS_CTID, FileType::Regular, 10)
    }

    fn mpath(s: &str) -> NonRootMPath {
        NonRootMPath::new(s).unwrap()
    }

    #[test]
    fn test_blocked_in_any_push() {
        let config = setup_config();
        let mp = mpath("all/1");
        let r = block_unacceptable_patterns(
            &config,
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Rejected(_)));

        let r = block_unacceptable_patterns(
            &config,
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Rejected(_)));
    }

    #[test]
    fn test_denied_only_in_native_push() {
        let config = setup_config();
        let mp = mpath("native/1");
        let r = block_unacceptable_patterns(
            &config,
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Rejected(_)));

        let r = block_unacceptable_patterns(
            &config,
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Accepted));
    }

    #[test]
    fn test_remove_denied_only_in_native_push() {
        let config = setup_config();
        let mp = mpath("native/1");
        let r =
            block_unacceptable_patterns(&config, &mp, CrossRepoPushSource::NativeToThisRepo, None)
                .unwrap();
        assert!(matches!(r, HookExecution::Rejected(_)));

        let r = block_unacceptable_patterns(
            &config,
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Accepted));
    }

    #[test]
    fn test_allowed_in_any_push() {
        let config = setup_config();
        let mp = mpath("ababagalamaga");
        let r = block_unacceptable_patterns(
            &config,
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Accepted));

        let r = block_unacceptable_patterns(
            &config,
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(matches!(r, HookExecution::Accepted));
    }
}
