/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use context::CoreContext;
use hook_manager::HookRepo;
use metaconfig_types::HookConfig;
use mononoke_types::BasicFileChange;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use serde::Deserialize;

use crate::CrossRepoPushSource;
use crate::FileHook;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::PushAuthoredBy;

#[derive(Debug, Deserialize, Clone)]
pub struct NoExecutableBinariesConfig {
    /// Message to include in the hook rejection if an executable binary file is
    /// is committed.
    /// ${filename} => The path of the file along with the filename
    illegal_executable_binary_message: String,
    /// Allow-list all files under any of these paths
    allow_list_paths: Option<Vec<String>>,
    /// Allow-list specific files that might be present in multiple paths
    /// by adding their Sha256 digest and size to this list.
    allow_list_files: Option<Vec<(String, u64)>>,
    /// If true, block ALL executables (including text-based scripts).
    /// If false or None (default), only block binary executables.
    block_all_executables: Option<bool>,
}

/// Hook to block commits containing files with illegal name patterns
#[derive(Clone, Debug)]
pub struct NoExecutableBinariesHook {
    config: NoExecutableBinariesConfig,
}

impl NoExecutableBinariesHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        let config = config
            .parse_options()
            .context("Missing or invalid JSON hook configuration for no-executable-files hook")?;
        Ok(Self::with_config(config))
    }

    pub fn with_config(config: NoExecutableBinariesConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl FileHook for NoExecutableBinariesHook {
    async fn run<'this: 'change, 'ctx: 'this, 'change, 'repo: 'change, 'path: 'change>(
        &'this self,
        ctx: &'ctx CoreContext,
        hook_repo: &'repo HookRepo,
        change: Option<&'change BasicFileChange>,
        path: &'path NonRootMPath,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if let Some(allow_list_paths) = &self.config.allow_list_paths {
            for allowed_path in allow_list_paths {
                let allowed_mpath = NonRootMPath::new(allowed_path)
                    .with_context(|| anyhow!("{allowed_path} is an invalid path"))?;
                if allowed_mpath.is_prefix_of(path) {
                    return Ok(HookExecution::Accepted);
                }
            }
        }
        let (content_id, size) = match change {
            Some(basic_fc) => {
                if basic_fc.file_type() != FileType::Executable {
                    // Not an executable, so passes hook right away
                    return Ok(HookExecution::Accepted);
                };
                (basic_fc.content_id(), basic_fc.size())
            }
            _ => {
                // File change is not committed, so passes hook
                return Ok(HookExecution::Accepted);
            }
        };

        let content_metadata = hook_repo.get_file_metadata(ctx, content_id).await?;

        let is_allow_listed_file =
            self.config
                .allow_list_files
                .as_ref()
                .map_or(false, |allow_listed_files| {
                    allow_listed_files.contains(&(content_metadata.sha256.to_string(), size))
                });

        if is_allow_listed_file {
            // Allow-listed file
            return Ok(HookExecution::Accepted);
        }

        if content_metadata.is_binary || self.config.block_all_executables.unwrap_or(false) {
            return Ok(HookExecution::Rejected(HookRejectionInfo::new_long(
                "Illegal executable file",
                self.config
                    .illegal_executable_binary_message
                    .replace("${filename}", &path.to_string()),
            )));
        }
        Ok(HookExecution::Accepted)
    }
}

#[cfg(test)]
mod test {

    use std::collections::HashMap;
    use std::collections::HashSet;

    use anyhow::anyhow;
    use blobstore::Loadable;
    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use hook_manager_testlib::HookTestRepo;
    use maplit::hashmap;
    use maplit::hashset;
    use mononoke_macros::mononoke;
    use mononoke_types::BonsaiChangeset;
    use tests_utils::CreateCommitContext;

    use super::*;

    /// Create default test config that each test can customize.
    fn make_test_config() -> NoExecutableBinariesConfig {
        NoExecutableBinariesConfig {
            illegal_executable_binary_message: "Executable file '${filename}' can't be committed."
                .to_string(),
            allow_list_paths: Some(vec!["some_dir/".to_string()]),
            allow_list_files: Some(vec![(
                "560a153deec1d4cda8481e96756e53c466f3c8eb2dabaf93f9e167c986bb77c4".to_string(),
                3,
            )]),
            block_all_executables: None,
        }
    }

    async fn test_setup(
        fb: FacebookInit,
    ) -> (
        CoreContext,
        HookTestRepo,
        HookRepo,
        NoExecutableBinariesHook,
    ) {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb)
            .await
            .expect("Failed to create test repo");
        let hook_repo = HookRepo::build_from(&repo);
        let config = make_test_config();
        let hook = NoExecutableBinariesHook::with_config(config);

        (ctx, repo, hook_repo, hook)
    }

    async fn assert_hook_execution(
        ctx: &CoreContext,
        hook_repo: HookRepo,
        bcs: BonsaiChangeset,
        hook: NoExecutableBinariesHook,
        valid_files: HashSet<&str>,
        illegal_files: HashMap<&str, &str>,
    ) -> Result<()> {
        for (path, change) in bcs.file_changes() {
            let hook_execution = hook
                .run(
                    ctx,
                    &hook_repo,
                    change.simplify(),
                    path,
                    CrossRepoPushSource::NativeToThisRepo,
                    PushAuthoredBy::User,
                )
                .await?;

            match hook_execution {
                HookExecution::Accepted => assert!(valid_files.contains(path.to_string().as_str())),
                HookExecution::Rejected(info) => {
                    let expected_info_msg = illegal_files
                        .get(path.to_string().as_str())
                        .ok_or(anyhow!("Unexpected rejected file"))?;
                    assert_eq!(info.long_description, expected_info_msg.to_string())
                }
            }
        }

        Ok(())
    }

    /// Test that the hook rejects an executable binary file
    #[mononoke::fbinit_test]
    async fn test_reject_single_executable_binary(fb: FacebookInit) -> Result<()> {
        let (ctx, repo, hook_repo, hook) = test_setup(fb).await;

        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file_with_type(
                "foo/bar/exec",
                vec![b'\0', 0x4D, 0x5A],
                FileType::Executable,
            )
            .add_file("bar/baz/hoo.txt", "a")
            .add_file("foo bar/baz", "b")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let valid_files: HashSet<&str> = hashset! {"foo bar/baz", "bar/baz/hoo.txt" };

        let illegal_files: HashMap<&str, &str> =
            hashmap! {"foo/bar/exec" => "Executable file 'foo/bar/exec' can't be committed."};

        assert_hook_execution(ctx, hook_repo, bcs, hook, valid_files, illegal_files).await
    }

    /// Test that the hook rejects multiple executable binaries
    #[mononoke::fbinit_test]
    async fn test_reject_multiple_executable_binaries(fb: FacebookInit) -> Result<()> {
        let (ctx, repo, hook_repo, hook) = test_setup(fb).await;

        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file_with_type(
                "foo/bar/exec",
                vec![b'\0', 0x4D, 0x5A],
                FileType::Executable,
            )
            .add_file_with_type(
                "foo/bar/another_exec",
                vec![0xB0, b'\0', 0x5A],
                FileType::Executable,
            )
            .add_file("bar/baz/hoo.txt", "a")
            .add_file("foo bar/baz", "b")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let valid_files: HashSet<&str> = hashset! {"foo bar/baz", "bar/baz/hoo.txt" };

        let illegal_files: HashMap<&str, &str> = hashmap! {
            "foo/bar/exec" => "Executable file 'foo/bar/exec' can't be committed.",
            "foo/bar/another_exec" => "Executable file 'foo/bar/another_exec' can't be committed."
        };

        assert_hook_execution(ctx, hook_repo, bcs, hook, valid_files, illegal_files).await
    }

    /// That that non-executable binaries pass
    #[mononoke::fbinit_test]
    async fn test_non_executable_binaries_pass(fb: FacebookInit) -> Result<()> {
        let (ctx, repo, hook_repo, hook) = test_setup(fb).await;

        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("foo/bar/exec", vec![b'\0', 0x4D, 0x5A])
            .add_file("bar/baz/hoo.txt", "a")
            .add_file("foo bar/baz", "b")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let valid_files: HashSet<&str> =
            hashset! {"foo/bar/exec", "foo bar/baz", "bar/baz/hoo.txt" };

        let illegal_files: HashMap<&str, &str> = hashmap! {};

        assert_hook_execution(ctx, hook_repo, bcs, hook, valid_files, illegal_files).await
    }

    /// That that executable scripts pass
    #[mononoke::fbinit_test]
    async fn test_executable_scripts_pass(fb: FacebookInit) -> Result<()> {
        let (ctx, repo, hook_repo, hook) = test_setup(fb).await;

        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("foo/bar/baz", "a")
            .add_file("foo bar/quux", "b")
            .add_file_with_type("bar/baz/hoo.txt", "c", FileType::Executable)
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let valid_files: HashSet<&str> =
            hashset! {"foo/bar/baz", "foo bar/quux", "bar/baz/hoo.txt" };

        let illegal_files: HashMap<&str, &str> = hashmap! {};

        assert_hook_execution(ctx, hook_repo, bcs, hook, valid_files, illegal_files).await
    }

    /// That that changes without executable file types are still allowed
    #[mononoke::fbinit_test]
    async fn test_changes_without_binaries_pass(fb: FacebookInit) -> Result<()> {
        let (ctx, repo, hook_repo, hook) = test_setup(fb).await;

        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("foo/bar/baz", "a")
            .add_file("foo bar/quux", "b")
            .add_file("bar/baz/hoo.txt", "c")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let valid_files: HashSet<&str> =
            hashset! {"foo/bar/baz", "foo bar/quux", "bar/baz/hoo.txt" };

        let illegal_files: HashMap<&str, &str> = hashmap! {};

        assert_hook_execution(ctx, hook_repo, bcs, hook, valid_files, illegal_files).await
    }

    /// Test that the hook allows executable binaries under allow-listed paths
    #[mononoke::fbinit_test]
    async fn test_executable_binaries_under_allow_listed_path_pass(fb: FacebookInit) -> Result<()> {
        let (ctx, repo, hook_repo, hook) = test_setup(fb).await;

        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file_with_type(
                "some_dir/exec",
                vec![b'\0', 0x4D, 0x5A],
                FileType::Executable,
            )
            .add_file("bar/baz/hoo.txt", "a")
            .add_file("foo bar/baz", "b")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let valid_files: HashSet<&str> =
            hashset! {"some_dir/exec", "foo bar/baz", "bar/baz/hoo.txt" };

        let illegal_files: HashMap<&str, &str> = hashmap! {};

        assert_hook_execution(ctx, hook_repo, bcs, hook, valid_files, illegal_files).await
    }

    /// Test that the hook allows executable binaries allow-listed by sha256 and
    /// size, regardless of its path.
    #[mononoke::fbinit_test]
    async fn test_executable_binaries_allow_listed_by_sha256_and_size_pass(
        fb: FacebookInit,
    ) -> Result<()> {
        let (ctx, repo, hook_repo, hook) = test_setup(fb).await;

        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file_with_type(
                "random_dir/always_allowed_file",
                vec![b'\0', 0x8D, 0x5F],
                FileType::Executable,
            )
            .add_file("bar/baz/hoo.txt", "a")
            .add_file("foo bar/baz", "b")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let valid_files: HashSet<&str> =
            hashset! {"random_dir/always_allowed_file", "foo bar/baz", "bar/baz/hoo.txt" };

        let illegal_files: HashMap<&str, &str> = hashmap! {};

        assert_hook_execution(ctx, hook_repo, bcs, hook, valid_files, illegal_files).await
    }

    /// Test that the hook rejects executable scripts when block_all_executables is true
    #[mononoke::fbinit_test]
    async fn test_reject_executable_scripts_when_block_all_enabled(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb)
            .await
            .expect("Failed to create test repo");
        let hook_repo = HookRepo::build_from(&repo);

        // Create config with block_all_executables enabled
        let config = NoExecutableBinariesConfig {
            illegal_executable_binary_message: "Executable file '${filename}' can't be committed."
                .to_string(),
            allow_list_paths: None,
            allow_list_files: None,
            block_all_executables: Some(true),
        };
        let hook = NoExecutableBinariesHook::with_config(config);

        borrowed!(ctx, repo);

        // Create a commit with an executable text file (script)
        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file_with_type(
                "scripts/my_script.sh",
                "#!/bin/bash\necho hello",
                FileType::Executable,
            )
            .add_file("regular_file.txt", "just text")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;

        let valid_files: HashSet<&str> = hashset! {"regular_file.txt"};

        let illegal_files: HashMap<&str, &str> = hashmap! {
            "scripts/my_script.sh" => "Executable file 'scripts/my_script.sh' can't be committed."
        };

        assert_hook_execution(ctx, hook_repo, bcs, hook, valid_files, illegal_files).await
    }
}
