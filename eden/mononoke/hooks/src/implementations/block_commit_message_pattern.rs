/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Context;
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
use crate::HookFileContentProvider;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BlockCommitMessagePatternConfig {
    /// Pattern to search for.  If found in any text file or the commit
    /// message, the commit is blocked.
    #[serde(with = "serde_regex")]
    pub(crate) pattern: Regex,

    /// Message to include in the hook rejection.  The string is expanded with
    /// the capture groups from the pattern, i.e. `${1}` is replaced with the
    /// first capture group, etc.
    pub(crate) message: String,
}

/// Hook to block commits based on matching a pattern in the commit message.
#[derive(Clone, Debug)]
pub struct BlockCommitMessagePatternHook {
    config: BlockCommitMessagePatternConfig,
}

impl BlockCommitMessagePatternHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        match &config.options {
            Some(options) => {
                let config = serde_json::from_str(options).context("Invalid hook config")?;
                Self::with_config(config)
            }
            None => bail!("Missing hook options"),
        }
    }

    pub fn with_config(config: BlockCommitMessagePatternConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl ChangesetHook for BlockCommitMessagePatternHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'fetcher: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _content_manager: &'fetcher dyn HookFileContentProvider,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }
        if let Some(caps) = self.config.pattern.captures(changeset.message()) {
            let mut message = String::new();
            caps.expand(&self.config.message, &mut message);
            return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Commit message contains blocked pattern",
                message,
            )));
        }
        Ok(HookExecution::Accepted)
    }
}

#[cfg(test)]
mod tests {
    use fbinit::FacebookInit;
    use tests_utils::bookmark;
    use tests_utils::drawdag::changes;
    use tests_utils::drawdag::create_from_dag_with_changes;
    use tests_utils::BasicTestRepo;

    use super::*;
    use crate::testlib::test_changeset_hook;

    #[fbinit::test]
    async fn test_blocks_pattern_when_present(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb).await?;

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
            message: String::from("disallowed marker: $1"),
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
            HookExecution::Accepted,
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
            HookExecution::Rejected(HookRejectionInfo {
                description: "Commit message contains blocked pattern".into(),
                long_description: "disallowed marker: %block_commit%".into(),
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
            HookExecution::Rejected(HookRejectionInfo {
                description: "Commit message contains blocked pattern".into(),
                long_description: "disallowed marker: %PREVENT_COMMIT%".into(),
            }),
        );

        Ok(())
    }
}
