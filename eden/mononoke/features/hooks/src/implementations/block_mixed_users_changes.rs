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
use crate::HookRejectionInfo;
use crate::HookRepo;
use crate::PushAuthoredBy;

#[derive(Deserialize, Clone, Debug)]
pub struct BlockMixedUsersChangesConfig {
    #[serde(default = "default_users_prefix")]
    users_prefix: String,
}

fn default_users_prefix() -> String {
    "users/".to_string()
}

#[derive(Clone, Debug)]
pub struct BlockMixedUsersChangesHook {
    config: BlockMixedUsersChangesConfig,
}

impl BlockMixedUsersChangesHook {
    pub fn new(config: &HookConfig) -> Result<Self> {
        Self::with_config(config.parse_options()?)
    }

    pub fn with_config(config: BlockMixedUsersChangesConfig) -> Result<Self> {
        Ok(Self { config })
    }
}

#[async_trait]
impl ChangesetHook for BlockMixedUsersChangesHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _hook_repo: &'repo HookRepo,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _cross_repo_push_source: CrossRepoPushSource,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        if push_authored_by.service() {
            return Ok(HookExecution::accepted());
        }

        let mut has_users_changes = false;
        let mut has_non_users_changes = false;

        for (path, _fc) in changeset.file_changes() {
            if path.to_string().starts_with(&self.config.users_prefix) {
                has_users_changes = true;
            } else {
                has_non_users_changes = true;
            }

            if has_users_changes && has_non_users_changes {
                return Ok(HookExecution::rejected(HookRejectionInfo::new_long(
                    "Mixed users/ and non-users/ changes",
                    format!(
                        "Your commit contains changes in {0} and non-{0}, making this commit unrevertable by anyone but you. This adds friction to incident resolution, so we encourage you to split the {0} changes into a separate diff.",
                        self.config.users_prefix,
                    ),
                )));
            }
        }

        Ok(HookExecution::accepted())
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

    fn default_config() -> BlockMixedUsersChangesConfig {
        BlockMixedUsersChangesConfig {
            users_prefix: "users/".to_string(),
        }
    }

    async fn run_hook(
        ctx: &CoreContext,
        repo: &HookTestRepo,
        bcs: &BonsaiChangeset,
        config: BlockMixedUsersChangesConfig,
        push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution, Error> {
        let hook_repo = HookRepo::build_from(repo);
        let hook = BlockMixedUsersChangesHook::with_config(config)?;
        hook.run(
            ctx,
            &hook_repo,
            &BookmarkKey::new("book")?,
            bcs,
            CrossRepoPushSource::NativeToThisRepo,
            push_authored_by,
        )
        .await
    }

    #[mononoke::fbinit_test]
    async fn test_only_users_changes(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("users/alice/test.txt", "sandbox code")
            .add_file("users/bob/lib.rs", "more sandbox")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let result = run_hook(ctx, repo, &bcs, default_config(), PushAuthoredBy::User).await?;
        assert_eq!(result, HookExecution::accepted());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_only_non_users_changes(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("src/lib.rs", "production code")
            .add_file("README.md", "docs")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let result = run_hook(ctx, repo, &bcs, default_config(), PushAuthoredBy::User).await?;
        assert_eq!(result, HookExecution::accepted());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_mixed_changes_rejected(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("users/alice/test.txt", "sandbox code")
            .add_file("src/lib.rs", "production code")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let result = run_hook(ctx, repo, &bcs, default_config(), PushAuthoredBy::User).await?;
        assert!(result.is_rejected());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_service_push_bypass(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("users/alice/test.txt", "sandbox code")
            .add_file("src/lib.rs", "production code")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let result = run_hook(ctx, repo, &bcs, default_config(), PushAuthoredBy::Service).await?;
        assert_eq!(result, HookExecution::accepted());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_empty_changeset(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let cs_id = CreateCommitContext::new_root(ctx, repo).commit().await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let result = run_hook(ctx, repo, &bcs, default_config(), PushAuthoredBy::User).await?;
        assert_eq!(result, HookExecution::accepted());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_deletion_in_users_with_non_users_change(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let parent = CreateCommitContext::new_root(ctx, repo)
            .add_file("users/alice/old.txt", "old content")
            .commit()
            .await?;

        let cs_id = CreateCommitContext::new(ctx, repo, vec![parent])
            .delete_file("users/alice/old.txt")
            .add_file("src/new.rs", "new production code")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let result = run_hook(ctx, repo, &bcs, default_config(), PushAuthoredBy::User).await?;
        assert!(result.is_rejected());
        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_custom_prefix(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        let config = BlockMixedUsersChangesConfig {
            users_prefix: "sandbox/".to_string(),
        };

        let cs_id = CreateCommitContext::new_root(ctx, repo)
            .add_file("sandbox/alice/test.txt", "sandbox code")
            .add_file("src/lib.rs", "production code")
            .commit()
            .await?;

        let bcs = cs_id.load(ctx, &repo.repo_blobstore).await?;
        let result = run_hook(ctx, repo, &bcs, config, PushAuthoredBy::User).await?;
        assert!(result.is_rejected());
        Ok(())
    }
}
