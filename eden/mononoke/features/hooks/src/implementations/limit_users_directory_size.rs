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
use serde::Deserialize;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRepo;
use crate::PushAuthoredBy;

/// Hook that limits the size of user directories in fbsource.
///
/// Designed for the `users/<shard>/<unixname>` directory structure
/// (e.g. `users/mo/mononoke`). Given a configured prefix like `users`,
/// the hook traverses two levels deep and checks the recursive size of
/// each depth-2 directory (`<unixname>`), NOT the depth-1 shard directory.
/// This prevents individual user directories from growing too large due
/// to RADAR rollout, which creates per-user working directories under
/// this path.
#[allow(dead_code)]
#[derive(Deserialize, Clone, Debug)]
pub struct LimitUsersDirectorySizeConfig {
    /// Max allowed recursive size in bytes for any directory at depth 2
    /// below each configured prefix.
    directory_size_limit: u64,

    /// Directory prefixes to monitor. For each prefix, directories at
    /// depth 2 are enumerated and their recursive sizes checked.
    /// e.g. ["foo"] -> checks foo/X/Y/ for all X, Y
    path_prefixes: Vec<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub struct LimitUsersDirectorySizeHook {
    config: LimitUsersDirectorySizeConfig,
}

#[allow(dead_code)]
impl LimitUsersDirectorySizeHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(config.parse_options()?)
    }

    pub fn with_config(config: LimitUsersDirectorySizeConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl ChangesetHook for LimitUsersDirectorySizeHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _hook_repo: &'repo HookRepo,
        _bookmark: &BookmarkKey,
        _changeset: &'cs BonsaiChangeset,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        Ok(HookExecution::Accepted)
    }
}

#[cfg(test)]
mod test {
    use anyhow::Error;
    use blobstore::Loadable;
    use borrowed::borrowed;
    use fbinit::FacebookInit;
    use hook_manager::HookRepo;
    use hook_manager_testlib::HookTestRepo;
    use mononoke_macros::mononoke;
    use tests_utils::CreateCommitContext;

    use super::*;

    fn make_test_config() -> LimitUsersDirectorySizeConfig {
        LimitUsersDirectorySizeConfig {
            directory_size_limit: 500,
            path_prefixes: vec!["prefix".to_string()],
        }
    }

    async fn run_hook(
        ctx: &CoreContext,
        hook_repo: &HookRepo,
        config: LimitUsersDirectorySizeConfig,
        changeset: &BonsaiChangeset,
    ) -> Result<HookExecution, Error> {
        let hook = LimitUsersDirectorySizeHook::with_config(config)?;
        hook.run(
            ctx,
            hook_repo,
            &BookmarkKey::new("book")?,
            changeset,
            CrossRepoPushSource::NativeToThisRepo,
            PushAuthoredBy::User,
        )
        .await
    }

    #[mononoke::fbinit_test]
    async fn test_depth2_under_limit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("prefix/a/b/file1", "small")
            .add_file("prefix/a/b/file2", "data")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let hook_repo = HookRepo::build_from(&repo);

        let config = make_test_config();
        let result = run_hook(ctx, &hook_repo, config, &bcs).await?;
        assert_eq!(result, HookExecution::Accepted);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_depth2_over_limit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        // Create a file large enough to exceed the 500 byte limit
        let large_content = "x".repeat(501);
        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("prefix/a/b/bigfile", large_content.as_str())
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let hook_repo = HookRepo::build_from(&repo);

        let config = make_test_config();
        let result = run_hook(ctx, &hook_repo, config, &bcs).await?;
        // TODO: Should reject once the hook is implemented
        assert_eq!(result, HookExecution::Accepted);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_checks_depth2_not_depth1(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        // Files nested deeper under depth-2 dir "b" -- hook checks recursive
        // size of "b", which includes deep/file.
        let large_content = "x".repeat(501);
        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("prefix/a/b/deep/file", large_content.as_str())
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let hook_repo = HookRepo::build_from(&repo);

        let config = make_test_config();
        let result = run_hook(ctx, &hook_repo, config, &bcs).await?;
        // TODO: Should reject once the hook is implemented
        assert_eq!(result, HookExecution::Accepted);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_nonexistent_prefix(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        // Large file outside the configured prefix -- should be accepted
        let large_content = "x".repeat(501);
        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("other/a/b/bigfile", large_content.as_str())
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let hook_repo = HookRepo::build_from(&repo);

        let config = make_test_config();
        let result = run_hook(ctx, &hook_repo, config, &bcs).await?;
        assert_eq!(result, HookExecution::Accepted);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_multiple_depth2_dirs(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let large_content = "x".repeat(501);
        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("prefix/a/small_dir/file1", "tiny")
            .add_file("prefix/a/big_dir/bigfile", large_content.as_str())
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let hook_repo = HookRepo::build_from(&repo);

        let config = make_test_config();
        let result = run_hook(ctx, &hook_repo, config, &bcs).await?;
        // TODO: Should reject big_dir once the hook is implemented
        assert_eq!(result, HookExecution::Accepted);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_no_short_circuit_service_push(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let large_content = "x".repeat(501);
        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("prefix/a/b/bigfile", large_content.as_str())
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let hook_repo = HookRepo::build_from(&repo);

        let config = make_test_config();
        let hook = LimitUsersDirectorySizeHook::with_config(config)?;

        // TODO: Service push should be rejected once the hook is implemented
        let result = hook
            .run(
                ctx,
                &hook_repo,
                &BookmarkKey::new("book")?,
                &bcs,
                CrossRepoPushSource::NativeToThisRepo,
                PushAuthoredBy::Service,
            )
            .await?;
        assert_eq!(result, HookExecution::Accepted);

        // TODO: Push-redirected should also be rejected once the hook is implemented
        let result = hook
            .run(
                ctx,
                &hook_repo,
                &BookmarkKey::new("book")?,
                &bcs,
                CrossRepoPushSource::PushRedirected,
                PushAuthoredBy::User,
            )
            .await?;
        assert_eq!(result, HookExecution::Accepted);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_incremental_growth_over_limit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        // First commit: directory size is 499 bytes, just under the 500 byte limit
        let content_under = "x".repeat(499);
        let cs_id1 = CreateCommitContext::new_root(ctx, repo)
            .add_file("prefix/a/b/file1", content_under.as_str())
            .commit()
            .await?;

        let bcs1 = cs_id1.load(ctx, &repo.repo_blobstore).await?;
        let hook_repo = HookRepo::build_from(&repo);

        let config = make_test_config();
        let result = run_hook(ctx, &hook_repo, config, &bcs1).await?;
        assert_eq!(result, HookExecution::Accepted);

        // Second commit: adds another file, pushing total over 500 bytes
        let cs_id2 = CreateCommitContext::new(ctx, repo, vec![cs_id1])
            .add_file("prefix/a/b/file2", "extra")
            .commit()
            .await?;

        let bcs2 = cs_id2.load(ctx, &repo.repo_blobstore).await?;

        let config = make_test_config();
        let result = run_hook(ctx, &hook_repo, config, &bcs2).await?;
        // TODO: Should reject once the hook is implemented
        assert_eq!(result, HookExecution::Accepted);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_depth2_dirs_independent_limits(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        // Two depth-2 directories each under the 500 byte limit individually,
        // but their combined size exceeds it. Should be accepted since each
        // directory is checked independently (not summed at depth-1).
        let content = "x".repeat(300);
        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("prefix/a/dir1/file", content.as_str())
            .add_file("prefix/a/dir2/file", content.as_str())
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let hook_repo = HookRepo::build_from(&repo);

        let config = make_test_config();
        let result = run_hook(ctx, &hook_repo, config, &bcs).await?;
        assert_eq!(result, HookExecution::Accepted);

        Ok(())
    }
}
