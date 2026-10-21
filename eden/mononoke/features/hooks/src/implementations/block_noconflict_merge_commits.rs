/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use context::CoreContext;
use mononoke_types::BonsaiChangeset;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookRepo;
use crate::PushAuthoredBy;
use crate::Pushvars;

/// Mercurial extra that `sl merge --noconflict` sets on the merge commits it
/// creates. Such a commit only records that its parents can be merged
/// automatically, so that stacked-diff tooling can depend on both of them; it
/// carries no content of its own and is not meant to be published.
const NOCONFLICT_MERGE_EXTRA: &str = "noconflict_merge";

#[derive(Clone, Debug)]
pub struct BlockNoconflictMergeCommitsHook;

impl BlockNoconflictMergeCommitsHook {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ChangesetHook for BlockNoconflictMergeCommitsHook {
    async fn run<'this: 'cs, 'ctx: 'this, 'cs, 'repo: 'cs>(
        &'this self,
        _ctx: &'ctx CoreContext,
        _hook_repo: &'repo HookRepo,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
        _maybe_pushvars: Option<&'cs Pushvars>,
    ) -> Result<HookExecution, Error> {
        let marked = changeset.is_merge()
            && changeset
                .hg_extra()
                .any(|(key, value)| key == NOCONFLICT_MERGE_EXTRA && value == b"1");
        if marked {
            Ok(HookExecution::rejected(HookRejectionInfo::new_long(
                "Conflict-free merge commit is not allowed",
                "This merge was created with 'sl merge --noconflict'. It only records that its parents can be merged automatically and must not be landed; land its parents and descendants instead.".to_string(),
            )))
        } else {
            Ok(HookExecution::accepted())
        }
    }
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use fbinit::FacebookInit;
    use hook_manager_testlib::HookTestRepo;
    use mononoke_macros::mononoke;
    use tests_utils::CreateCommitContext;
    use tests_utils::bookmark;

    use super::*;
    use crate::testlib::test_changeset_hook;

    #[mononoke::fbinit_test]
    async fn test_rejects_only_marked_merges(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(fb).await?;

        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("a", "a")
            .commit()
            .await?;
        let left = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("b", "b")
            .commit()
            .await?;
        let right = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file("c", "c")
            .commit()
            .await?;
        let plain_merge = CreateCommitContext::new(&ctx, &repo, vec![left, right])
            .commit()
            .await?;
        let marked_merge = CreateCommitContext::new(&ctx, &repo, vec![left, right])
            .add_extra(NOCONFLICT_MERGE_EXTRA, b"1".to_vec())
            .commit()
            .await?;
        let marked_non_merge = CreateCommitContext::new(&ctx, &repo, vec![left])
            .add_file("d", "d")
            .add_extra(NOCONFLICT_MERGE_EXTRA, b"1".to_vec())
            .commit()
            .await?;
        bookmark(&ctx, &repo, "main")
            .create_publishing(root)
            .await?;

        let hook = BlockNoconflictMergeCommitsHook::new();
        let run = |cs_id, push_authored_by| {
            test_changeset_hook(
                &ctx,
                &repo,
                &hook,
                "main",
                cs_id,
                CrossRepoPushSource::NativeToThisRepo,
                push_authored_by,
            )
        };

        assert_eq!(
            run(left, PushAuthoredBy::User).await?,
            HookExecution::accepted()
        );
        assert_eq!(
            run(plain_merge, PushAuthoredBy::User).await?,
            HookExecution::accepted()
        );
        assert_eq!(
            run(marked_non_merge, PushAuthoredBy::User).await?,
            HookExecution::accepted()
        );
        let rejected = HookExecution::rejected(HookRejectionInfo::new_long(
            "Conflict-free merge commit is not allowed",
            "This merge was created with 'sl merge --noconflict'. It only records that its parents can be merged automatically and must not be landed; land its parents and descendants instead.".to_string(),
        ));
        assert_eq!(run(marked_merge, PushAuthoredBy::User).await?, rejected);
        // Landing services push on behalf of users; the marker is still
        // decisive for them.
        assert_eq!(run(marked_merge, PushAuthoredBy::Service).await?, rejected);

        Ok(())
    }
}
