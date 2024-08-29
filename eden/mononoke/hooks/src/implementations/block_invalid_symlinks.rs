/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::BasicFileChange;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use regex::Regex;
use serde::Deserialize;

use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookStateProvider;
use crate::PushAuthoredBy;

const DEFAULT_MAX_SYMLINK_SIZE: u64 = 1024;

#[derive(Deserialize, Clone, Debug, Default)]
pub struct BlockInvalidSymlinksConfig {
    /// Max size for symlink contents. Defaults to 1024 (bytes).
    #[serde(default)]
    max_size: Option<u64>,

    /// Paths matching these regexes will be ignored.
    #[serde(default, with = "serde_regex")]
    ignore_path_regexes: Vec<Regex>,

    /// Allow newlines in symlink contents.
    #[serde(default)]
    allow_newlines: bool,

    /// Allow null bytes in symlink contents.
    #[serde(default)]
    allow_nulls: bool,

    /// Allow empty symlinks.
    #[serde(default)]
    allow_empty: bool,
}

/// Hook to block commits with invalid symlinks.
#[derive(Clone, Debug)]
pub struct BlockInvalidSymlinksHook {
    config: BlockInvalidSymlinksConfig,
}

impl BlockInvalidSymlinksHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(config.parse_options()?)
    }

    pub fn with_config(config: BlockInvalidSymlinksConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl FileHook for BlockInvalidSymlinksHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        content_manager: &'fetcher dyn HookStateProvider,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        let path = path.to_string();

        if self
            .config
            .ignore_path_regexes
            .iter()
            .any(|regex| regex.is_match(&path))
        {
            return Ok(HookExecution::Accepted);
        }

        match change {
            Some(change) if change.file_type() == FileType::Symlink => {
                let max_size = self.config.max_size.unwrap_or(DEFAULT_MAX_SYMLINK_SIZE);
                if change.size() > max_size {
                    return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                        "symlink too long",
                        format!(
                            "symlink '{}' contents are too long ({} > {})",
                            path,
                            change.size(),
                            max_size
                        ),
                    )));
                }

                if !self.config.allow_empty && change.size() == 0 {
                    return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                        "symlink is empty",
                        format!("symlink '{path}' has no contents"),
                    )));
                }

                if let Some(text) = content_manager
                    .get_file_text(ctx, change.content_id())
                    .await?
                {
                    if !self.config.allow_newlines && text.contains(&b'\n') {
                        return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                            "symlink contains newline",
                            format!("symlink '{path}' contents contain a newline character"),
                        )));
                    } else if !self.config.allow_nulls && text.contains(&b'\0') {
                        return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                            "symlink contains null byte",
                            format!("symlink '{path}' contents contain a null byte"),
                        )));
                    }
                }

                Ok(HookExecution::Accepted)
            }
            _ => Ok(HookExecution::Accepted),
        }
    }
}

#[cfg(test)]
mod tests {
    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;
    use tests_utils::bookmark;
    use tests_utils::drawdag::changes;
    use tests_utils::drawdag::create_from_dag_with_changes;
    use tests_utils::BasicTestRepo;

    use super::*;
    use crate::testlib::test_file_hook;

    #[mononoke::fbinit_test]
    async fn test_block_invalid_symlinks(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb).await?;

        let changesets = create_from_dag_with_changes(
            &ctx,
            &repo,
            r##"
                A-B-C-D-E-F
            "##,
            changes! {
                "A" => |c| c.add_file_with_type("A", "okay", FileType::Symlink),
                "B" => |c| c.add_file_with_type("B", "", FileType::Symlink),
                "C" => |c| c.add_file_with_type("C", "a".repeat(DEFAULT_MAX_SYMLINK_SIZE as usize + 1), FileType::Symlink),
                "D" => |c| c.add_file_with_type("D", "dont\ndothis", FileType::Symlink),
                "E" => |c| c.add_file_with_type("E", "dont\x00dothis", FileType::Symlink),
            },
        )
        .await?;
        bookmark(&ctx, &repo, "main")
            .create_publishing(changesets["E"])
            .await?;

        let enabled_hook = BlockInvalidSymlinksHook::with_config(BlockInvalidSymlinksConfig {
            ignore_path_regexes: vec![],
            max_size: None,
            allow_newlines: false,
            allow_nulls: false,
            allow_empty: false,
        })?;

        // Hooks with each setting toggled separately.
        let ignore_paths_hook =
            BlockInvalidSymlinksHook::with_config(BlockInvalidSymlinksConfig {
                ignore_path_regexes: vec![Regex::new(r".*")?],
                ..Default::default()
            })?;
        let high_size_limit_hook =
            BlockInvalidSymlinksHook::with_config(BlockInvalidSymlinksConfig {
                max_size: Some(2048),
                ..Default::default()
            })?;
        let allow_newline_hook =
            BlockInvalidSymlinksHook::with_config(BlockInvalidSymlinksConfig {
                allow_newlines: true,
                ..Default::default()
            })?;
        let allow_null_hook = BlockInvalidSymlinksHook::with_config(BlockInvalidSymlinksConfig {
            allow_nulls: true,
            ..Default::default()
        })?;
        let allow_empty_hook = BlockInvalidSymlinksHook::with_config(BlockInvalidSymlinksConfig {
            allow_empty: true,
            ..Default::default()
        })?;

        // Okay symlink
        assert_eq!(
            test_file_hook(
                &ctx,
                &repo,
                &enabled_hook,
                changesets["A"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            vec![("A".try_into()?, HookExecution::Accepted)]
        );

        // Empty symlink
        assert_eq!(
            test_file_hook(
                &ctx,
                &repo,
                &enabled_hook,
                changesets["B"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            vec![(
                "B".try_into()?,
                HookExecution::Rejected(HookRejectionInfo {
                    description: "symlink is empty".into(),
                    long_description: "symlink 'B' has no contents".into(),
                }),
            )]
        );
        assert_eq!(
            test_file_hook(
                &ctx,
                &repo,
                &allow_empty_hook,
                changesets["B"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            vec![("B".try_into()?, HookExecution::Accepted)],
        );

        // Large symlink
        assert_eq!(
            test_file_hook(
                &ctx,
                &repo,
                &enabled_hook,
                changesets["C"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            vec![(
                "C".try_into()?,
                HookExecution::Rejected(HookRejectionInfo {
                    description: "symlink too long".into(),
                    long_description: "symlink 'C' contents are too long (1025 > 1024)".into(),
                }),
            )]
        );
        assert_eq!(
            test_file_hook(
                &ctx,
                &repo,
                &high_size_limit_hook,
                changesets["C"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            vec![("C".try_into()?, HookExecution::Accepted)],
        );
        assert_eq!(
            test_file_hook(
                &ctx,
                &repo,
                &ignore_paths_hook,
                changesets["C"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            vec![("C".try_into()?, HookExecution::Accepted)],
        );

        // Newline
        assert_eq!(
            test_file_hook(
                &ctx,
                &repo,
                &enabled_hook,
                changesets["D"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            vec![(
                "D".try_into()?,
                HookExecution::Rejected(HookRejectionInfo {
                    description: "symlink contains newline".into(),
                    long_description: "symlink 'D' contents contain a newline character".into(),
                }),
            )]
        );
        assert_eq!(
            test_file_hook(
                &ctx,
                &repo,
                &allow_newline_hook,
                changesets["D"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            vec![("D".try_into()?, HookExecution::Accepted)],
        );

        // Null byte
        assert_eq!(
            test_file_hook(
                &ctx,
                &repo,
                &enabled_hook,
                changesets["E"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            vec![(
                "E".try_into()?,
                HookExecution::Rejected(HookRejectionInfo {
                    description: "symlink contains null byte".into(),
                    long_description: "symlink 'E' contents contain a null byte".into(),
                }),
            )]
        );
        assert_eq!(
            test_file_hook(
                &ctx,
                &repo,
                &allow_null_hook,
                changesets["E"],
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::User,
            )
            .await?,
            vec![("E".try_into()?, HookExecution::Accepted)],
        );

        Ok(())
    }
}
