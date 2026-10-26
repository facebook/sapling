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
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::hook_messages::HookMessages;
use crate::hook_messages::hook_messages;

hook_messages! {
    /// Messages of the `block_files` hook.
    pub(crate) struct BlockFilesMessages for "block_files" {
        /// The path of a changed file matches a block pattern.
        blocked(
            /// Path of the file.
            path,
            /// The block regex it matched.
            pattern,
        ) = "Blocked filename '${path}' matched block pattern '${pattern}'. This path is protected and your change must not modify it. To fix this, revert your changes to '${path}' so that it no longer appears in your diff, then re-submit.";
    }
}

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
    messages: BlockFilesMessages,
}

impl BlockFilesHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Ok(Self {
            messages: BlockFilesMessages::new(config),
            ..Self::with_config(config.parse_options()?)?
        })
    }

    pub fn with_config(config: BlockFilesConfig) -> Result<Self> {
        Ok(Self {
            config,
            messages: BlockFilesMessages::default(),
        })
    }
}

#[async_trait]
impl FileHook for BlockFilesHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'repo: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _hook_repo: &'repo HookRepo,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::accepted());
        }

        block_unacceptable_patterns(
            &self.config,
            &self.messages,
            path,
            cross_repo_push_source,
            change,
        )
    }
}

fn rejection(path: &str, pattern: &Regex, messages: &BlockFilesMessages) -> HookExecution {
    HookExecution::rejected(
        messages
            .blocked(path, pattern)
            .into_rejection("Blocked filename matched name pattern"),
    )
}

/// Easily-testable business logic of the `BlockFiles` hook
fn block_unacceptable_patterns<'a, 'b>(
    config: &BlockFilesConfig,
    messages: &BlockFilesMessages,
    path: &'a NonRootMPath,
    cross_repo_push_source: CrossRepoPushSource,
    change: Option<&'b BasicFileChange>,
) -> Result<HookExecution> {
    let path = path.to_string();
    if CrossRepoPushSource::NativeToThisRepo == cross_repo_push_source {
        for pattern in &config.native_push_only_block_patterns {
            if pattern.is_match(&path) {
                return Ok(rejection(&path, pattern, messages));
            }
        }
    }
    if change.is_none() {
        // It is acceptable to delete any file
        return Ok(HookExecution::accepted());
    }

    for pattern in &config.block_patterns {
        if pattern.is_match(&path) {
            return Ok(rejection(&path, pattern, messages));
        }
    }

    Ok(HookExecution::accepted())
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;
    use mononoke_types::FileType;
    use mononoke_types::GitLfs;
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
        BasicFileChange::new(TWOS_CTID, FileType::Regular, 10, GitLfs::FullContent)
    }

    fn mpath(s: &str) -> NonRootMPath {
        NonRootMPath::new(s).unwrap()
    }

    #[mononoke::test]
    fn test_blocked_in_any_push() {
        let config = setup_config();
        let mp = mpath("all/1");
        let r = block_unacceptable_patterns(
            &config,
            &BlockFilesMessages::default(),
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(r.is_rejected());

        let r = block_unacceptable_patterns(
            &config,
            &BlockFilesMessages::default(),
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(r.is_rejected());
    }

    #[mononoke::test]
    fn test_denied_only_in_native_push() {
        let config = setup_config();
        let mp = mpath("native/1");
        let r = block_unacceptable_patterns(
            &config,
            &BlockFilesMessages::default(),
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(r.is_rejected());

        let r = block_unacceptable_patterns(
            &config,
            &BlockFilesMessages::default(),
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(r.is_accepted());
    }

    #[mononoke::test]
    fn test_remove_denied_only_in_native_push() {
        let config = setup_config();
        let mp = mpath("native/1");
        let r = block_unacceptable_patterns(
            &config,
            &BlockFilesMessages::default(),
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            None,
        )
        .unwrap();
        assert!(r.is_rejected());

        let r = block_unacceptable_patterns(
            &config,
            &BlockFilesMessages::default(),
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(r.is_accepted());
    }

    #[mononoke::test]
    fn test_allowed_in_any_push() {
        let config = setup_config();
        let mp = mpath("ababagalamaga");
        let r = block_unacceptable_patterns(
            &config,
            &BlockFilesMessages::default(),
            &mp,
            CrossRepoPushSource::NativeToThisRepo,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(r.is_accepted());

        let r = block_unacceptable_patterns(
            &config,
            &BlockFilesMessages::default(),
            &mp,
            CrossRepoPushSource::PushRedirected,
            Some(&basic_change()),
        )
        .unwrap();
        assert!(r.is_accepted());
    }
}
