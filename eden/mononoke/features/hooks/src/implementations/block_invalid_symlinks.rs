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
use crate::HookRepo;
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

    /// Block symlinks that point to absolute paths in paths matching these
    /// regexes. If empty, absolute symlink blocking is disabled. Use ".*" to
    /// block absolute symlinks in all paths (subject to ignore_path_regexes).
    #[serde(default, with = "serde_regex")]
    block_absolute_symlinks_path_regexes: Vec<Regex>,

    /// Dry run mode for block_absolute_symlinks_path_regexes. When enabled,
    /// absolute symlinks will be logged to Scuba but not rejected. This allows
    /// testing the hook's behavior before fully enabling it.
    #[serde(default)]
    block_absolute_symlinks_dry_run: bool,
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

/// Check if a symlink target is an absolute path.
/// Handles both Unix ("/foo") and Windows ("C:\foo", "\\server\share") styles.
fn is_absolute_symlink(target: &[u8]) -> bool {
    match target {
        // Unix absolute path
        [b'/', ..] => true,
        // Windows drive letter (e.g., C:\foo or C:/foo)
        [drive, b':', sep, ..]
            if drive.is_ascii_alphabetic() && (*sep == b'\\' || *sep == b'/') =>
        {
            true
        }
        // Windows UNC path (e.g., \\server\share)
        [b'\\', b'\\', ..] => true,
        _ => false,
    }
}

#[async_trait]
impl FileHook for BlockInvalidSymlinksHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'repo: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        hook_repo: &'repo HookRepo,
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

                if let Some(text) = hook_repo.get_file_bytes(ctx, change.content_id()).await? {
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

                    // Check if this path should be checked for absolute symlinks.
                    // Only call is_absolute_symlink when the path matches a regex,
                    // to avoid unnecessary work when blocking is not applicable.
                    let path_matches_absolute_check =
                        !self.config.block_absolute_symlinks_path_regexes.is_empty()
                            && self
                                .config
                                .block_absolute_symlinks_path_regexes
                                .iter()
                                .any(|regex| regex.is_match(&path));

                    if path_matches_absolute_check && is_absolute_symlink(&text) {
                        if self.config.block_absolute_symlinks_dry_run {
                            // Dry run mode: log absolute symlinks to Scuba without blocking
                            ctx.scuba().clone().log_with_msg(
                                "block_absolute_symlinks dry run: would reject",
                                format!(
                                    "symlink '{}' points to an absolute path which will break builds. \
                                     Use a relative path instead.",
                                    path
                                ),
                            );
                        } else {
                            // Blocking mode: reject absolute symlinks
                            return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                                "symlink uses absolute path",
                                format!(
                                    "symlink '{}' points to an absolute path which will break builds. \
                                     Use a relative path instead.",
                                    path
                                ),
                            )));
                        }
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
    use hook_manager_testlib::HookTestRepo;
    use mononoke_macros::mononoke;
    use tests_utils::bookmark;
    use tests_utils::drawdag::changes;
    use tests_utils::drawdag::create_from_dag_with_changes;

    use super::*;
    use crate::testlib::test_file_hook;

    #[mononoke::fbinit_test]
    async fn test_block_invalid_symlinks(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(fb).await?;

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
            block_absolute_symlinks_dry_run: false,
            block_absolute_symlinks_path_regexes: vec![],
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

    #[mononoke::fbinit_test]
    async fn test_block_absolute_symlinks(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(fb).await?;

        let changesets = create_from_dag_with_changes(
            &ctx,
            &repo,
            r##"
                A-B-C-D-E-F
            "##,
            changes! {
                "A" => |c| c.add_file_with_type("www/foo/link", "/absolute/target", FileType::Symlink),
                "B" => |c| c.add_file_with_type("www/bar/link", "../relative/target", FileType::Symlink),
                "C" => |c| c.add_file_with_type("ignored/link", "/absolute/target", FileType::Symlink),
                "D" => |c| c.add_file_with_type("www/win/link", "C:\\Windows\\target", FileType::Symlink),
                "E" => |c| c.add_file_with_type("www/unc/link", "\\\\server\\share", FileType::Symlink),
                "F" => |c| c.add_file_with_type("fbcode/link", "/absolute/target", FileType::Symlink),
            },
        )
        .await?;
        bookmark(&ctx, &repo, "main")
            .create_publishing(changesets["F"])
            .await?;

        // Hook that blocks absolute symlinks everywhere
        let hook = BlockInvalidSymlinksHook::with_config(BlockInvalidSymlinksConfig {
            block_absolute_symlinks_path_regexes: vec![Regex::new(r".*")?],
            ..Default::default()
        })?;

        // Hook that blocks absolute symlinks with ignore_path_regexes
        let hook_with_ignore = BlockInvalidSymlinksHook::with_config(BlockInvalidSymlinksConfig {
            block_absolute_symlinks_path_regexes: vec![Regex::new(r".*")?],
            ignore_path_regexes: vec![Regex::new(r"^ignored/")?],
            ..Default::default()
        })?;

        // Hook that only blocks absolute symlinks in www/ paths
        let hook_www_only = BlockInvalidSymlinksHook::with_config(BlockInvalidSymlinksConfig {
            block_absolute_symlinks_path_regexes: vec![Regex::new(r"^www/")?],
            ..Default::default()
        })?;

        // Absolute symlink -> rejected
        let results = test_file_hook(
            &ctx,
            &repo,
            &hook,
            changesets["A"],
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await?;
        assert!(
            results.contains(&(
                "www/foo/link".try_into()?,
                HookExecution::Rejected(HookRejectionInfo {
                    description: "symlink uses absolute path".into(),
                    long_description:
                        "symlink 'www/foo/link' points to an absolute path which will break builds. \
                         Use a relative path instead."
                            .into(),
                }),
            )),
            "absolute symlink should be rejected, got: {:?}",
            results
        );

        // Relative symlink -> accepted
        let results = test_file_hook(
            &ctx,
            &repo,
            &hook,
            changesets["B"],
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await?;
        assert!(
            results.contains(&("www/bar/link".try_into()?, HookExecution::Accepted)),
            "relative symlink should be accepted, got: {:?}",
            results
        );

        // Absolute symlink in ignored path -> accepted
        let results = test_file_hook(
            &ctx,
            &repo,
            &hook_with_ignore,
            changesets["C"],
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await?;
        assert!(
            results.contains(&("ignored/link".try_into()?, HookExecution::Accepted)),
            "absolute symlink in ignored path should be accepted, got: {:?}",
            results
        );

        // Windows drive letter absolute symlink -> rejected
        let results = test_file_hook(
            &ctx,
            &repo,
            &hook,
            changesets["D"],
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await?;
        assert!(
            results.contains(&(
                "www/win/link".try_into()?,
                HookExecution::Rejected(HookRejectionInfo {
                    description: "symlink uses absolute path".into(),
                    long_description:
                        "symlink 'www/win/link' points to an absolute path which will break builds. \
                         Use a relative path instead."
                            .into(),
                }),
            )),
            "Windows drive letter absolute symlink should be rejected, got: {:?}",
            results
        );

        // Windows UNC path absolute symlink -> rejected
        let results = test_file_hook(
            &ctx,
            &repo,
            &hook,
            changesets["E"],
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await?;
        assert!(
            results.contains(&(
                "www/unc/link".try_into()?,
                HookExecution::Rejected(HookRejectionInfo {
                    description: "symlink uses absolute path".into(),
                    long_description:
                        "symlink 'www/unc/link' points to an absolute path which will break builds. \
                         Use a relative path instead."
                            .into(),
                }),
            )),
            "Windows UNC absolute symlink should be rejected, got: {:?}",
            results
        );

        // Absolute symlink in www/ with path regex -> rejected
        let results = test_file_hook(
            &ctx,
            &repo,
            &hook_www_only,
            changesets["A"],
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await?;
        assert!(
            results.contains(&(
                "www/foo/link".try_into()?,
                HookExecution::Rejected(HookRejectionInfo {
                    description: "symlink uses absolute path".into(),
                    long_description:
                        "symlink 'www/foo/link' points to an absolute path which will break builds. \
                         Use a relative path instead."
                            .into(),
                }),
            )),
            "absolute symlink in www/ should be rejected with path regex, got: {:?}",
            results
        );

        // Absolute symlink outside www/ with path regex -> accepted
        let results = test_file_hook(
            &ctx,
            &repo,
            &hook_www_only,
            changesets["F"],
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await?;
        assert!(
            results.contains(&("fbcode/link".try_into()?, HookExecution::Accepted)),
            "absolute symlink outside www/ should be accepted with path regex, got: {:?}",
            results
        );

        Ok(())
    }
}
