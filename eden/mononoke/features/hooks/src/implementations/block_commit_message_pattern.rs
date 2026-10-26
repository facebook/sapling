/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use context::CoreContext;
use mononoke_types::BonsaiChangeset;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::Pushvars;
use crate::hook_messages::HookMessages;
use crate::hook_messages::hook_messages;

hook_messages! {
    /// Messages of the `block_commit_message_pattern` hook.
    pub(crate) struct BlockCommitMessagePatternMessages for "block_commit_message_pattern" {
        /// The commit message matches `pattern`.
        blocked(
            /// The `pattern` regex.
            pattern,
        ) with captures = "The commit message matches the blocked pattern '${pattern}'.";
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlockCommitMessagePatternConfig {
    /// Pattern to search for.  If found in any text file or the commit
    /// message, the commit is blocked.
    #[serde(with = "serde_regex")]
    pattern: Regex,

    /// Replaces the `blocked` message. Use the hook config's `messages`
    /// instead.
    #[serde(default)]
    message: Option<String>,
}

/// Hook to block commits based on matching a pattern in the commit message.
#[derive(Clone, Debug)]
pub struct BlockCommitMessagePatternHook {
    pattern: Regex,
    messages: BlockCommitMessagePatternMessages,
}

impl BlockCommitMessagePatternHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_options(config.parse_options()?, config)
    }

    fn with_options(options: BlockCommitMessagePatternConfig, config: &HookConfig) -> Result<Self> {
        let messages = BlockCommitMessagePatternMessages::builder()
            .captures(&options.pattern)?
            .legacy("blocked", options.message.as_deref())
            .build(config);
        Ok(Self {
            pattern: options.pattern,
            messages,
        })
    }

    #[cfg(test)]
    fn with_config(options: BlockCommitMessagePatternConfig) -> Self {
        Self::with_options(options, &HookConfig::default()).expect("valid config")
    }
}

#[async_trait]
impl ChangesetHook for BlockCommitMessagePatternHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _hook_repo: &'repo HookRepo,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
        _maybe_pushvars: Option<&'cs Pushvars>,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::accepted());
        }
        if let Some(captures) = self.pattern.captures(changeset.message()) {
            return Ok(HookExecution::rejected(
                self.messages
                    .blocked(&self.pattern, &captures)
                    .into_rejection("Commit message contains blocked pattern"),
            ));
        }
        Ok(HookExecution::accepted())
    }
}

#[cfg(test)]
mod tests {
    use fbinit::FacebookInit;
    use hook_manager_testlib::HookTestRepo;
    use mononoke_macros::mononoke;
    use tests_utils::bookmark;
    use tests_utils::drawdag::changes;
    use tests_utils::drawdag::create_from_dag_with_changes;

    use super::*;
    use crate::HookRejectionInfo;
    use crate::testlib::test_changeset_hook;

    #[mononoke::fbinit_test]
    async fn test_blocks_pattern_when_present(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(fb).await?;

        let changesets = create_from_dag_with_changes(
            &ctx,
            &repo,
            r##"
                Z-A-B-C
            "##,
            changes! {
                "B" => |c| c.set_message("B\n%block_commit%"),
                "C" => |c| c.set_message("C\n%PREVENT_COMMIT%"),
            },
        )
        .await?;
        bookmark(&ctx, &repo, "main")
            .create_publishing(changesets["Z"])
            .await?;

        let hook = BlockCommitMessagePatternHook::with_config(BlockCommitMessagePatternConfig {
            pattern: Regex::new(r"(?i)(%(block_commit|prevent_commit)%)")?,
            message: Some(String::from("disallowed marker: $1")),
        });

        assert_eq!(
            test_changeset_hook(
                &ctx,
                &repo,
                &hook,
                "main",
                changesets["A"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            HookExecution::accepted(),
        );
        assert_eq!(
            test_changeset_hook(
                &ctx,
                &repo,
                &hook,
                "main",
                changesets["B"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            HookExecution::rejected(HookRejectionInfo {
                description: "Commit message contains blocked pattern".into(),
                long_description: "disallowed marker: %block_commit%".into(),
                message: None,
            }),
        );
        assert_eq!(
            test_changeset_hook(
                &ctx,
                &repo,
                &hook,
                "main",
                changesets["C"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            HookExecution::rejected(HookRejectionInfo {
                description: "Commit message contains blocked pattern".into(),
                long_description: "disallowed marker: %PREVENT_COMMIT%".into(),
                message: None,
            }),
        );

        Ok(())
    }

    /// A replacement in the `messages` map can insert capture groups by name.
    #[mononoke::fbinit_test]
    async fn test_message_with_named_group(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(fb).await?;

        let changesets = create_from_dag_with_changes(
            &ctx,
            &repo,
            r##"
                Z-A
            "##,
            changes! {
                "A" => |c| c.set_message("A\n%block_commit%"),
            },
        )
        .await?;
        bookmark(&ctx, &repo, "main")
            .create_publishing(changesets["Z"])
            .await?;

        let hook = BlockCommitMessagePatternHook::new(&HookConfig {
            options: Some(r#"{"pattern": "%(?P<marker>block_commit|prevent_commit)%"}"#.to_owned()),
            messages: [(
                "blocked".to_owned(),
                "Remove the ${marker} marker ($0).".to_owned(),
            )]
            .into(),
            ..Default::default()
        })?;

        assert_eq!(
            test_changeset_hook(
                &ctx,
                &repo,
                &hook,
                "main",
                changesets["A"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            HookExecution::rejected(HookRejectionInfo {
                description: "Commit message contains blocked pattern".into(),
                long_description: "Remove the block_commit marker (%block_commit%).".into(),
                message: None,
            }),
        );

        Ok(())
    }

    /// A pattern whose named group clashes with the `pattern` parameter is
    /// rejected.
    #[mononoke::test]
    fn test_clashing_group_is_rejected() {
        let config = HookConfig {
            options: Some(r#"{"pattern": "(?P<pattern>block_commit)"}"#.to_owned()),
            ..Default::default()
        };
        assert!(BlockCommitMessagePatternHook::new(&config).is_err());
    }
}
