/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

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

    /// Block relative symlinks whose target -- after being resolved against
    /// the symlink's parent directory -- falls outside one of these path
    /// prefixes. The first prefix that the symlink path is under is enforced.
    /// e.g. `["www/"]` rejects any symlink under `www/` whose resolved target
    /// does not also stay under `www/`. Useful for repos where build hosts
    /// only check out a subtree and cross-subtree symlinks become dangling on
    /// the build host.
    #[serde(default)]
    block_escaping_relative_symlinks: Vec<String>,

    /// Dry run mode for block_escaping_relative_symlinks. When enabled,
    /// escaping relative symlinks will be logged to Scuba but not rejected.
    #[serde(default)]
    block_escaping_relative_symlinks_dry_run: bool,
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

/// Resolve `target` as a relative symlink stored at `file_path` and report
/// whether the resolved path falls outside `root_path`. Resolution is purely
/// syntactic: `.` is dropped, `..` pops the previous component, and an extra
/// `..` past the start of the path is treated as an escape.
///
/// Caller is expected to filter absolute targets via `is_absolute_symlink`
/// first; any unexpected absolute or prefix component encountered here is
/// also treated as an escape (conservative).
fn relative_target_escapes_root(target: &[u8], file_path: &str, root_path: &str) -> bool {
    let parent = Path::new(file_path).parent().unwrap_or(Path::new(""));
    let target_path = Path::new(OsStr::from_bytes(target));

    let mut normalized = PathBuf::new();
    let mut poppable: usize = 0;
    for component in parent.join(target_path).components() {
        match component {
            Component::Normal(c) => {
                normalized.push(c);
                poppable += 1;
            }
            Component::ParentDir => {
                if poppable > 0 {
                    normalized.pop();
                    poppable -= 1;
                } else {
                    return true;
                }
            }
            Component::CurDir => {}
            Component::RootDir | Component::Prefix(_) => return true,
        }
    }

    !normalized.starts_with(root_path)
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
                                    "symlink '{path}' points to an absolute path which will break builds. \
                                     Use a relative path instead."
                                ),
                            );
                        } else {
                            // Blocking mode: reject absolute symlinks
                            return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                                "symlink uses absolute path",
                                format!(
                                    "symlink '{path}' points to an absolute path which will break builds. \
                                     Use a relative path instead."
                                ),
                            )));
                        }
                    }

                    // Check whether a relative symlink resolves outside its
                    // configured root subtree. Skip if the target is absolute
                    // (already handled above) or if the symlink path is not
                    // under any configured root.
                    if !is_absolute_symlink(&text) {
                        let escape_root = self
                            .config
                            .block_escaping_relative_symlinks
                            .iter()
                            .find(|root| Path::new(&path).starts_with(root.as_str()));

                        if let Some(root) = escape_root {
                            if relative_target_escapes_root(&text, &path, root) {
                                let target_str = String::from_utf8_lossy(&text);
                                let message = format!(
                                    "symlink '{path}' resolves to a target outside '{root}' which will \
                                     break builds on hosts that only check out '{root}'. \
                                     Symlink target was '{target_str}'.",
                                );
                                if self.config.block_escaping_relative_symlinks_dry_run {
                                    ctx.scuba().clone().log_with_msg(
                                        "block_escaping_relative_symlinks dry run: would reject",
                                        message,
                                    );
                                } else {
                                    return Ok(HookExecution::Rejected(
                                        HookRejectionInfo::new_long(
                                            "symlink escapes containment root",
                                            message,
                                        ),
                                    ));
                                }
                            }
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
            block_escaping_relative_symlinks: vec![],
            block_escaping_relative_symlinks_dry_run: false,
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
            "absolute symlink should be rejected, got: {results:?}"
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
            "relative symlink should be accepted, got: {results:?}"
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
            "absolute symlink in ignored path should be accepted, got: {results:?}"
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
            "Windows drive letter absolute symlink should be rejected, got: {results:?}"
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
            "Windows UNC absolute symlink should be rejected, got: {results:?}"
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
            "absolute symlink in www/ should be rejected with path regex, got: {results:?}"
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
            "absolute symlink outside www/ should be accepted with path regex, got: {results:?}"
        );

        Ok(())
    }

    #[mononoke::test]
    fn test_relative_target_escapes_root() {
        // Stays inside the same directory.
        assert!(!relative_target_escapes_root(
            b"sibling.py",
            "www/foo/bar/link",
            "www/",
        ));

        // Walks up but stays inside www/.
        assert!(!relative_target_escapes_root(
            b"../sibling.py",
            "www/foo/bar/link",
            "www/",
        ));
        assert!(!relative_target_escapes_root(
            b"../../sibling.py",
            "www/foo/bar/link",
            "www/",
        ));

        // Walks up just past www/ -- escapes.
        assert!(relative_target_escapes_root(
            b"../../../other/x.py",
            "www/foo/bar/link",
            "www/",
        ));

        // Mixed `..` then back into a sibling root -- still escapes www/.
        assert!(relative_target_escapes_root(
            b"../../../../other/y.py",
            "www/foo/bar/link",
            "www/",
        ));

        // `..` past the repo root -- escape.
        assert!(relative_target_escapes_root(
            b"../../../../../etc/passwd",
            "www/foo/bar/link",
            "www/",
        ));

        // `.` segments and `//` collapses are no-ops.
        assert!(!relative_target_escapes_root(
            b"./subdir/.//target.py",
            "www/foo/bar/link",
            "www/",
        ));

        // Symlink at the very top of www/ pointing to a sibling at the top.
        assert!(!relative_target_escapes_root(
            b"sibling", "www/link", "www/",
        ));

        // Symlink directly under www/ pointing one level up -- escapes.
        assert!(relative_target_escapes_root(
            b"../sibling",
            "www/link",
            "www/",
        ));

        // A round trip through `..` and back: www/foo/bar/link -> ../bar/x
        // resolves to www/foo/bar/x which stays within www/.
        assert!(!relative_target_escapes_root(
            b"../bar/x",
            "www/foo/bar/link",
            "www/",
        ));
    }

    #[mononoke::fbinit_test]
    async fn test_block_escaping_relative_symlinks(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(fb).await?;

        let changesets = create_from_dag_with_changes(
            &ctx,
            &repo,
            r##"
                A-B-C-D-E-F-G
            "##,
            changes! {
                // Relative symlink whose target resolves outside the
                // configured root subtree.
                "A" => |c| c.add_file_with_type(
                    "www/a/b/c/link.py",
                    "../../../../other/x.py",
                    FileType::Symlink,
                ),
                // Relative symlink that stays inside www/.
                "B" => |c| c.add_file_with_type(
                    "www/a/b/link",
                    "../c/target.py",
                    FileType::Symlink,
                ),
                // Symlink outside www/ -- no rule applies.
                "C" => |c| c.add_file_with_type(
                    "other/foo/link",
                    "../../www/x.py",
                    FileType::Symlink,
                ),
                // Symlink under an ignored path.
                "D" => |c| c.add_file_with_type(
                    "www/ignored/link",
                    "../../other/x.py",
                    FileType::Symlink,
                ),
                // Absolute symlink -- handled by the absolute check, not this one.
                "E" => |c| c.add_file_with_type(
                    "www/abs/link",
                    "/absolute/target",
                    FileType::Symlink,
                ),
                // `..` past the repo root.
                "F" => |c| c.add_file_with_type(
                    "www/x/link",
                    "../../../../../etc/passwd",
                    FileType::Symlink,
                ),
                // Same-directory relative symlink.
                "G" => |c| c.add_file_with_type(
                    "www/safe/link",
                    "sibling.py",
                    FileType::Symlink,
                ),
            },
        )
        .await?;
        bookmark(&ctx, &repo, "main")
            .create_publishing(changesets["G"])
            .await?;

        // Hook that rejects www/ symlinks escaping www/.
        let hook = BlockInvalidSymlinksHook::with_config(BlockInvalidSymlinksConfig {
            block_escaping_relative_symlinks: vec!["www/".to_string()],
            ..Default::default()
        })?;

        // Hook with ignore_path_regexes for an escape hatch.
        let hook_with_ignore = BlockInvalidSymlinksHook::with_config(BlockInvalidSymlinksConfig {
            block_escaping_relative_symlinks: vec!["www/".to_string()],
            ignore_path_regexes: vec![Regex::new(r"^www/ignored/")?],
            ..Default::default()
        })?;

        // Hook in dry-run mode -- escaping symlinks must still be accepted.
        let dry_run_hook = BlockInvalidSymlinksHook::with_config(BlockInvalidSymlinksConfig {
            block_escaping_relative_symlinks: vec!["www/".to_string()],
            block_escaping_relative_symlinks_dry_run: true,
            ..Default::default()
        })?;

        // Escaping relative symlink should be rejected.
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
            results.iter().any(|(p, e)| p.to_string() == "www/a/b/c/link.py"
                && matches!(
                    e,
                    HookExecution::Rejected(info) if info.description == "symlink escapes containment root"
                )),
            "escaping relative symlink should be rejected, got: {results:?}"
        );

        // Relative symlink that stays inside www/ -- accepted.
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
            results.contains(&("www/a/b/link".try_into()?, HookExecution::Accepted)),
            "in-tree relative symlink should be accepted, got: {results:?}"
        );

        // Symlink outside www/ doesn't match the rule -- accepted.
        let results = test_file_hook(
            &ctx,
            &repo,
            &hook,
            changesets["C"],
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await?;
        assert!(
            results.contains(&("other/foo/link".try_into()?, HookExecution::Accepted)),
            "non-matching path should be accepted, got: {results:?}"
        );

        // ignore_path_regexes provides an escape hatch.
        let results = test_file_hook(
            &ctx,
            &repo,
            &hook_with_ignore,
            changesets["D"],
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await?;
        assert!(
            results.contains(&("www/ignored/link".try_into()?, HookExecution::Accepted)),
            "ignored path should be accepted, got: {results:?}"
        );

        // Absolute symlinks fall through this check -- accepted because no
        // absolute-symlink rule is configured here.
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
            results.contains(&("www/abs/link".try_into()?, HookExecution::Accepted)),
            "absolute symlink should fall through this check, got: {results:?}"
        );

        // `..` past the repo root -- rejected.
        let results = test_file_hook(
            &ctx,
            &repo,
            &hook,
            changesets["F"],
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await?;
        assert!(
            results.iter().any(|(p, e)| p.to_string() == "www/x/link"
                && matches!(
                    e,
                    HookExecution::Rejected(info) if info.description == "symlink escapes containment root"
                )),
            "symlink escaping above repo root should be rejected, got: {results:?}"
        );

        // Same-directory symlink -- accepted.
        let results = test_file_hook(
            &ctx,
            &repo,
            &hook,
            changesets["G"],
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await?;
        assert!(
            results.contains(&("www/safe/link".try_into()?, HookExecution::Accepted)),
            "same-directory symlink should be accepted, got: {results:?}"
        );

        // Dry run mode -- escaping symlink is accepted instead of rejected.
        let results = test_file_hook(
            &ctx,
            &repo,
            &dry_run_hook,
            changesets["A"],
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await?;
        assert!(
            results.contains(&("www/a/b/c/link.py".try_into()?, HookExecution::Accepted,)),
            "dry-run mode should accept escaping symlink, got: {results:?}"
        );

        Ok(())
    }
}
