/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use anyhow::Result;
use async_trait::async_trait;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use context::CoreContext;
use derivation_queue_thrift::DerivationPriority;
use fsnodes::RootFsnodeId;
use futures::future;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use mononoke_types::BonsaiChangeset;
use mononoke_types::FsnodeId;
use mononoke_types::MPathElement;
use mononoke_types::NonRootMPath;
use mononoke_types::fsnode::FsnodeEntry;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use serde::Deserialize;

use crate::ChangesetHook;
use crate::CrossRepoPushSource;
use crate::HookConfig;
use crate::HookExecution;
use crate::HookRejectionInfo;
use crate::HookRepo;
use crate::PushAuthoredBy;

const MAX_CONCURRENCY: usize = 100;

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
    /// below the configured prefix.
    directory_size_limit: u64,

    /// Directory prefix to monitor. Directories at depth 2 below this
    /// prefix are checked against the size limit.
    /// e.g. "users" -> checks users/X/Y/ for all X, Y
    path_prefix: String,
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
        ctx: &'ctx CoreContext,
        hook_repo: &'repo HookRepo,
        _bookmark: &BookmarkKey,
        changeset: &'cs BonsaiChangeset,
        _cross_repo_push_source: CrossRepoPushSource,
        _push_authored_by: PushAuthoredBy,
    ) -> Result<HookExecution> {
        let prefix = NonRootMPath::new(self.config.path_prefix.as_bytes())?;
        let prefix_len = prefix.num_components();

        // Collect all unique depth-2 directories touched by this changeset,
        // grouped by their depth-2 root. A commit may touch multiple user
        // directories (e.g. prefix/a/b/* and prefix/x/y/*).
        let depth2_paths: HashSet<Vec<MPathElement>> = changeset
            .file_changes()
            .filter(|(path, fc)| {
                fc.is_changed()
                    && path.num_components() >= prefix_len + 3
                    && prefix.is_prefix_of(*path)
            })
            .map(|(path, _)| path.into_iter().take(prefix_len + 2).cloned().collect())
            .collect();

        if depth2_paths.is_empty() {
            return Ok(HookExecution::Accepted);
        }

        let cs_id = changeset.get_changeset_id();

        let root = hook_repo
            .repo_derived_data()
            .derive::<RootFsnodeId>(ctx, cs_id, DerivationPriority::LOW)
            .await?;

        let root_fsnode_id = root.into_fsnode_id();

        // Check all depth-2 directories concurrently with bounded parallelism.
        let rejection = stream::iter(depth2_paths)
            .map(|depth2_path| async move {
                check_dir_size(&self.config, ctx, hook_repo, &root_fsnode_id, &depth2_path).await
            })
            .buffer_unordered(MAX_CONCURRENCY)
            .try_filter_map(|x| future::ready(Ok(x)))
            .try_next()
            .await?;

        if let Some(rejection) = rejection {
            return Ok(rejection);
        }

        Ok(HookExecution::Accepted)
    }
}

async fn check_dir_size(
    config: &LimitUsersDirectorySizeConfig,
    ctx: &CoreContext,
    hook_repo: &HookRepo,
    root_fsnode_id: &FsnodeId,
    depth2_path: &[MPathElement],
) -> Result<Option<HookExecution>> {
    let blobstore = hook_repo.repo_blobstore_arc();
    let mut fsnode = root_fsnode_id.load(ctx, &blobstore).await?;

    // Navigate to the parent directory (prefix + shard level).
    for element in &depth2_path[..depth2_path.len() - 1] {
        match fsnode.lookup(element) {
            Some(FsnodeEntry::Directory(dir)) => {
                fsnode = dir.id().load(ctx, &blobstore).await?;
            }
            _ => return Ok(None),
        }
    }

    // Look up the depth-2 entry and check its recursive size.
    let depth2_name = &depth2_path[depth2_path.len() - 1];
    let size = match fsnode.lookup(depth2_name) {
        Some(FsnodeEntry::Directory(dir)) => dir.summary().descendant_files_total_size,
        _ => return Ok(None),
    };

    if size > config.directory_size_limit {
        let limit = config.directory_size_limit;
        let path = depth2_path
            .iter()
            .map(|e| String::from_utf8_lossy(e.as_ref()).into_owned())
            .collect::<Vec<_>>()
            .join("/");
        let size_mb = size / (1024 * 1024);
        let limit_mb = limit / (1024 * 1024);
        return Ok(Some(HookExecution::Rejected(HookRejectionInfo::new_long(
            "Directory too large",
            format!(
                "Directory '{}' is {} bytes ({} MB), which exceeds the \
                 size limit of {} bytes ({} MB). Please reduce the size \
                 of this directory before pushing.",
                path, size, size_mb, limit, limit_mb,
            ),
        ))));
    }

    Ok(None)
}

#[cfg(test)]
mod test {
    use anyhow::Error;
    use assert_matches::assert_matches;
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
            path_prefix: "prefix".to_string(),
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
        let info = assert_matches!(result, HookExecution::Rejected(info) => info);
        assert!(
            info.long_description.contains("prefix/a/b"),
            "Expected path prefix/a/b in rejection message, got: {}",
            info.long_description
        );

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
        let info = assert_matches!(result, HookExecution::Rejected(info) => info);
        assert!(
            info.long_description.contains("prefix/a/b"),
            "Should check depth-2 dir 'b' recursively, got: {}",
            info.long_description
        );

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
        let info = assert_matches!(result, HookExecution::Rejected(info) => info);
        assert!(
            info.long_description.contains("big_dir"),
            "Should reject the oversized dir, got: {}",
            info.long_description
        );

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

        // Service push should still be rejected (no short-circuit)
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
        assert_matches!(result, HookExecution::Rejected(_));

        // Push-redirected should also still be rejected
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
        assert_matches!(result, HookExecution::Rejected(_));

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
        let info = assert_matches!(result, HookExecution::Rejected(info) => info);
        assert!(
            info.long_description.contains("prefix/a/b"),
            "Expected path prefix/a/b in rejection message, got: {}",
            info.long_description
        );

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

    #[mononoke::fbinit_test]
    async fn test_file_update_not_double_counted(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: HookTestRepo = test_repo_factory::build_empty(ctx.fb).await?;
        borrowed!(ctx, repo);

        // Create a 400 byte file
        let content = "x".repeat(400);
        let cs_id1 = CreateCommitContext::new_root(ctx, repo)
            .add_file("prefix/a/b/file", content.as_str())
            .commit()
            .await?;

        let bcs1 = cs_id1.load(ctx, &repo.repo_blobstore).await?;
        let hook_repo = HookRepo::build_from(&repo);

        let config = make_test_config();
        let result = run_hook(ctx, &hook_repo, config, &bcs1).await?;
        assert_eq!(result, HookExecution::Accepted);

        // Update the same file to 401 bytes. Total directory size should be
        // 401 (not 400 + 401 = 801), so it should still be accepted.
        let updated_content = "x".repeat(401);
        let cs_id2 = CreateCommitContext::new(ctx, repo, vec![cs_id1])
            .add_file("prefix/a/b/file", updated_content.as_str())
            .commit()
            .await?;

        let bcs2 = cs_id2.load(ctx, &repo.repo_blobstore).await?;

        let config = make_test_config();
        let result = run_hook(ctx, &hook_repo, config, &bcs2).await?;
        assert_eq!(result, HookExecution::Accepted);

        Ok(())
    }
}
