/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
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

#[derive(Debug, Deserialize, Clone)]
pub struct NoBadFilenamesConfig {
    /// Regex representing the filename patterns that are allow listed
    #[serde(default, with = "serde_regex")]
    allowlist_regex: Option<Regex>,
    /// Regex representing the filename patterns that are illegal
    #[serde(with = "serde_regex")]
    illegal_regex: Regex,
    /// Message to include in the hook rejection if the filename matches the illegal pattern,
    /// with the following replacements
    /// ${filename} => The path of the file along with the filename
    /// ${illegal_pattern} => The illegal regex pattern that was matched
    illegal_filename_message: String,
}

/// Hook to block commits containing files with illegal name patterns
#[derive(Clone, Debug)]
pub struct NoBadFilenamesHook {
    config: NoBadFilenamesConfig,
}

impl NoBadFilenamesHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        let config = config
            .parse_options()
            .context("Missing or invalid JSON hook configuration for no-bad-filenames hook")?;
        Ok(Self::with_config(config))
    }

    pub fn with_config(config: NoBadFilenamesConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl FileHook for NoBadFilenamesHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'fetcher: 'change, 'path: 'change>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _content_manager: &'fetcher dyn HookFileContentProvider,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if push_authored_by.service() || change.is_none() {
            return Ok(HookExecution::Accepted);
        }

        let path = format!("{}", path);
        // Check if the path matches the illegal regex
        if self.config.illegal_regex.is_match(&path) {
            // Check if the path has been allowlisted
            if let Some(ref allow) = self.config.allowlist_regex
                && allow.is_match(&path)
            {
                Ok(HookExecution::Accepted)
            } else {
                Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                    "Illegal filename",
                    self.config
                        .illegal_filename_message
                        .replace("${filename}", &path)
                        .replace("${illegal_pattern}", &self.config.illegal_regex.to_string()),
                )))
            }
        } else {
            Ok(HookExecution::Accepted)
        }
    }
}

#[cfg(test)]
mod test {
    use anyhow::anyhow;
    use anyhow::Error;
    use blobstore::Loadable;
    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use repo_hook_file_content_provider::RepoHookFileContentProvider;
    use tests_utils::BasicTestRepo;
    use tests_utils::CreateCommitContext;

    use super::*;

    // Regex for filenames that are almost never supposed to be committed. It matches any
    // occurrence of backticks, pipes, and colon characters. It also matches tilde characters,
    // but only if they appear at the beginning or end of a file name or right before a dot
    // (e.g. right before the file extension).
    static BAD_FILENAMES_REGEX: &str = r"[`|:]|(^|/)~|~($|[/.])";

    /// Create default test config that each test can customize.
    fn make_test_config() -> NoBadFilenamesConfig {
        NoBadFilenamesConfig {
            allowlist_regex: None,
            illegal_regex: Regex::new(".*").unwrap(),
            illegal_filename_message: "Filename: '${filename}' and Pattern '${illegal_pattern}'."
                .to_string(),
        }
    }

    #[fbinit::test]
    async fn test_no_bad_filenames_hook_basic(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);
        let content_manager = RepoHookFileContentProvider::new(&repo);
        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("dir/a", "a")
            .add_file("dir/b", "b")
            .add_file("dir/c", "c")
            .commit()
            .await?;
        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let mut config = make_test_config();
        config.illegal_regex = Regex::new(BAD_FILENAMES_REGEX).unwrap();
        let hook = NoBadFilenamesHook::with_config(config);
        for (path, change) in bcs.file_changes() {
            let hook_execution = hook
                .run(
                    ctx,
                    &content_manager,
                    change.simplify(),
                    path,
                    CrossRepoPushSource::NativeToThisRepo,
                    PushAuthoredBy::User,
                )
                .await?;
            assert_eq!(hook_execution, HookExecution::Accepted);
        }
        Ok(())
    }

    #[fbinit::test]
    async fn test_no_bad_filenames_hook_illegal_filenames(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);
        let content_manager = RepoHookFileContentProvider::new(&repo);
        // Illegal file names
        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("foo/bar:baz/quux", "a")
            .add_file("foo/bar`baz/quux", "b")
            .add_file("foo/bar|baz/quux", "c")
            .add_file("~foo/bar/baz.txt", "d")
            .add_file("foo~/bar/baz.txt", "e")
            .add_file("foo/bar/~baz.txt", "f")
            .add_file("foo/bar/baz~.txt", "g")
            .add_file("foo/bar/baz.txt~", "h")
            .commit()
            .await?;
        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let mut config = make_test_config();
        config.illegal_regex = Regex::new(BAD_FILENAMES_REGEX).unwrap();
        let hook = NoBadFilenamesHook::with_config(config);
        for (path, change) in bcs.file_changes() {
            let hook_execution = hook
                .run(
                    ctx,
                    &content_manager,
                    change.simplify(),
                    path,
                    CrossRepoPushSource::NativeToThisRepo,
                    PushAuthoredBy::User,
                )
                .await?;
            match hook_execution {
                HookExecution::Accepted => return Err(anyhow!("should be rejected")),
                HookExecution::Rejected(info) => {
                    assert_eq!(
                        info.long_description,
                        "Filename: '${filename}' and Pattern '${illegal_pattern}'."
                            .replace("${filename}", path.to_string().as_str())
                            .replace("${illegal_pattern}", BAD_FILENAMES_REGEX)
                    )
                }
            }
        }
        Ok(())
    }

    #[fbinit::test]
    async fn test_no_bad_filenames_hook_fishy_but_legal_filenames(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);
        let content_manager = RepoHookFileContentProvider::new(&repo);
        // Illegal file names
        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("foo/bar/baz", "a")
            .add_file("foo bar/baz quux", "b")
            .add_file("foo~bar/baz~quux.txt", "c")
            .commit()
            .await?;
        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let mut config = make_test_config();
        config.illegal_regex = Regex::new(BAD_FILENAMES_REGEX).unwrap();
        let hook = NoBadFilenamesHook::with_config(config);
        for (path, change) in bcs.file_changes() {
            let hook_execution = hook
                .run(
                    ctx,
                    &content_manager,
                    change.simplify(),
                    path,
                    CrossRepoPushSource::NativeToThisRepo,
                    PushAuthoredBy::User,
                )
                .await?;
            assert_eq!(hook_execution, HookExecution::Accepted);
        }
        Ok(())
    }

    #[fbinit::test]
    async fn test_no_bad_filenames_hook_illegal_filenames_with_allowlist(
        fb: FacebookInit,
    ) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);
        let content_manager = RepoHookFileContentProvider::new(&repo);
        // Illegal file names
        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("foo/bar:baz/quux", "a")
            .add_file("foo/bar`baz/quux", "b")
            .add_file("foo/bar|baz/quux", "c")
            .commit()
            .await?;
        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let mut config = make_test_config();
        config.illegal_regex = Regex::new(BAD_FILENAMES_REGEX).unwrap();
        config.allowlist_regex = Some(Regex::new("foo/bar.*").unwrap());
        let hook = NoBadFilenamesHook::with_config(config);
        for (path, change) in bcs.file_changes() {
            let hook_execution = hook
                .run(
                    ctx,
                    &content_manager,
                    change.simplify(),
                    path,
                    CrossRepoPushSource::NativeToThisRepo,
                    PushAuthoredBy::User,
                )
                .await?;
            assert_eq!(hook_execution, HookExecution::Accepted);
        }
        Ok(())
    }
}
