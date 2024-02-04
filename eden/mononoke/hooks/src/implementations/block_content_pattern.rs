/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Write;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::BasicFileChange;
use mononoke_types::NonRootMPath;
use regex::Regex;
use serde::Deserialize;

use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookFileContentProvider;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;

#[derive(Deserialize, Clone, Debug)]
pub struct BlockContentPatternConfig {
    /// Pattern to search for.  If found in any text file or the commit
    /// message, the commit is blocked.
    #[serde(with = "serde_regex")]
    pub(crate) pattern: Regex,

    /// Message to include in the hook rejection.  The string is expanded with
    /// the capture groups from the pattern, i.e. `${1}` is replaced with the
    /// first capture group, etc.
    pub(crate) message: String,
}

/// Hook to block commits based on matching a pattern in modified file
/// contents.
///
/// This hook only applies to UTF-8 files.
#[derive(Clone, Debug)]
pub struct BlockContentPatternHook {
    config: BlockContentPatternConfig,
}

impl BlockContentPatternHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        match &config.options {
            Some(options) => {
                let config = serde_json::from_str(options).context("Invalid hook config")?;
                Self::with_config(config)
            }
            None => bail!("Missing hook options"),
        }
    }

    pub fn with_config(config: BlockContentPatternConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl FileHook for BlockContentPatternHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        content_manager: &'fetcher dyn HookFileContentProvider,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::Accepted);
        }
        if let Some(change) = change {
            if let Some(text) = content_manager
                .get_file_text(ctx, change.content_id())
                .await?
            {
                // Ignore non-UTF8 or binary files
                if let Ok(text) = std::str::from_utf8(&text) {
                    if let Some(caps) = self.config.pattern.captures(text) {
                        let mut message = String::new();
                        caps.expand(&self.config.message, &mut message);
                        write!(message, ": {}", path)?;
                        return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                            "File contains blocked pattern",
                            message,
                        )));
                    }
                }
            }
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
    use crate::testlib::test_file_hook;

    #[fbinit::test]
    async fn test_blocks_pattern_when_present(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb).await?;

        let changesets = create_from_dag_with_changes(
            &ctx,
            &repo,
            r##"
                Z-A-B-C-D
            "##,
            changes! {
                "B" => |c| c.add_file("file", "contains\n%block_commit%\ninside\n"),
                "C" => |c| c.add_file("file", "contains %PREVENT_COMMIT% inside\n"),
            },
        )
        .await?;
        bookmark(&ctx, &repo, "main")
            .create_publishing(changesets["Z"])
            .await?;

        let hook = BlockContentPatternHook::with_config(BlockContentPatternConfig {
            pattern: Regex::new(r"(?i)(%(block_commit|prevent_commit)%)")?,
            message: String::from("disallowed marker: $1"),
        })?;

        // Normal files are fine.
        assert_eq!(
            test_file_hook(
                &ctx,
                &repo,
                &hook,
                changesets["A"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            vec![("A".try_into()?, HookExecution::Accepted),]
        );
        assert_eq!(
            test_file_hook(
                &ctx,
                &repo,
                &hook,
                changesets["B"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            vec![
                ("B".try_into()?, HookExecution::Accepted),
                (
                    "file".try_into()?,
                    HookExecution::Rejected(HookRejectionInfo {
                        description: "File contains blocked pattern".into(),
                        long_description: "disallowed marker: %block_commit%: file".into(),
                    })
                )
            ],
        );
        assert_eq!(
            test_file_hook(
                &ctx,
                &repo,
                &hook,
                changesets["C"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            vec![
                ("C".try_into()?, HookExecution::Accepted),
                (
                    "file".try_into()?,
                    HookExecution::Rejected(HookRejectionInfo {
                        description: "File contains blocked pattern".into(),
                        long_description: "disallowed marker: %PREVENT_COMMIT%: file".into(),
                    })
                )
            ],
        );

        // Only modified files are checked, so D is fine despite B and C
        // adding files that contain the marker.
        assert_eq!(
            test_file_hook(
                &ctx,
                &repo,
                &hook,
                changesets["D"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            vec![("D".try_into()?, HookExecution::Accepted)],
        );

        Ok(())
    }
}
