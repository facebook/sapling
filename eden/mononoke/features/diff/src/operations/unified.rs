/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use context::CoreContext;
use futures::try_join;
use mononoke_api::MononokeRepo;
use mononoke_api::RepoContext;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;

use crate::error::DiffError;
use crate::types::DiffSingleInput;
use crate::types::UnifiedDiff;
use crate::types::UnifiedDiffOpts;
use crate::utils::content::DiffFileOpts;
use crate::utils::content::load_diff_file;

pub async fn unified<R: MononokeRepo>(
    ctx: &CoreContext,
    repo: &RepoContext<R>,
    base: Option<DiffSingleInput>,
    other: Option<DiffSingleInput>,
    options: UnifiedDiffOpts,
) -> Result<UnifiedDiff, DiffError> {
    let diff_file_opts = DiffFileOpts {
        file_type: options.file_type,
        inspect_lfs_pointers: options.inspect_lfs_pointers,
        omit_content: options.omit_content,
    };

    let (base_file, other_file) = try_join!(
        async {
            if let Some(base_input) = &base {
                let default_path =
                    to_non_root_path("base_path").context("The hardcoded path was not valid")?;
                load_diff_file(ctx, repo, base_input, default_path, &diff_file_opts).await
            } else {
                Ok(None)
            }
        },
        async {
            if let Some(other_input) = &other {
                let default_path =
                    to_non_root_path("other_path").expect("The hardcoded path was not valid");
                load_diff_file(ctx, repo, other_input, default_path, &diff_file_opts).await
            } else {
                Ok(None)
            }
        }
    )?;

    let is_binary = xdiff::file_is_binary(&base_file) || xdiff::file_is_binary(&other_file);

    let xdiff_opts = xdiff::DiffOpts::from(options);
    let raw_diff = xdiff::diff_unified(base_file, other_file, xdiff_opts);

    Ok(UnifiedDiff {
        raw_diff,
        is_binary,
    })
}

fn to_non_root_path(path: &str) -> Result<NonRootMPath, DiffError> {
    let mpath = MPath::new(path)?;
    let non_root_mpath = NonRootMPath::try_from(mpath)?;
    Ok(non_root_mpath)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fbinit::FacebookInit;
    use mononoke_api::Repo;
    use mononoke_api::RepoContext;
    use mononoke_macros::mononoke;
    use test_repo_factory;
    use tests_utils::CreateCommitContext;

    use super::*;
    use crate::types::DiffCopyInfo;
    use crate::types::DiffFileType;
    use crate::types::DiffInputChangesetPath;
    use crate::types::DiffSingleInput;

    async fn init_test_repo(ctx: &CoreContext) -> Result<RepoContext<Repo>, DiffError> {
        let repo: Repo = test_repo_factory::build_empty(ctx.fb)
            .await
            .map_err(DiffError::internal)?;
        let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo))
            .await
            .map_err(DiffError::internal)?;
        Ok(repo_ctx)
    }

    #[mononoke::fbinit_test]
    async fn test_unified_basic(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo_ctx = init_test_repo(&ctx).await?;

        // Create test commits with different content
        let base_cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .add_file("file.txt", "line1\nline2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, repo_ctx.repo(), vec![base_cs])
            .add_file("file.txt", "line1\nmodified line2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        // Test the unified diff function
        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: to_non_root_path("file.txt")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: to_non_root_path("file.txt")?,
            replacement_path: None,
        });

        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
        };

        let diff = unified(
            &ctx,
            &repo_ctx,
            Some(base_input),
            Some(other_input),
            options,
        )
        .await?;

        let diff_str = String::from_utf8_lossy(&diff.raw_diff);

        // The unified diff should contain the change we made
        assert!(diff_str.contains("-line2"));
        assert!(diff_str.contains("+modified line2"));
        assert!(diff_str.contains("@@ -1,3 +1,3 @@"));
        assert!(!diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_binary_files(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo_ctx = init_test_repo(&ctx).await?;

        // Create test commits with binary content (contains null bytes)
        let base_cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .add_file("binary.bin", b"binary\x00content\x01here".as_slice())
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, repo_ctx.repo(), vec![base_cs])
            .add_file("binary.bin", b"different\x00binary\x02data".as_slice())
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: to_non_root_path("binary.bin")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: to_non_root_path("binary.bin")?,
            replacement_path: None,
        });

        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
        };

        let diff = unified(
            &ctx,
            &repo_ctx,
            Some(base_input),
            Some(other_input),
            options,
        )
        .await?;

        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert_eq!(
            diff_str,
            "diff --git a/binary.bin b/binary.bin\nBinary file binary.bin has changed\n"
        );
        assert!(diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_empty_files(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo_ctx = init_test_repo(&ctx).await?;

        // Test with one empty file and one with content
        let base_cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, repo_ctx.repo(), vec![base_cs])
            .add_file("new_file.txt", "new content\nline2\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: to_non_root_path("new_file.txt")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: to_non_root_path("new_file.txt")?,
            replacement_path: None,
        });

        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
        };

        let diff = unified(
            &ctx,
            &repo_ctx,
            Some(base_input),
            Some(other_input),
            options,
        )
        .await?;

        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(diff_str.contains("+new content"));
        assert!(diff_str.contains("+line2"));
        assert!(diff_str.contains("@@ -0,0 +1,2 @@"));
        assert!(!diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_omit_content(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo_ctx = init_test_repo(&ctx).await?;

        let base_cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .add_file("file.txt", "line1\nline2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, repo_ctx.repo(), vec![base_cs])
            .add_file("file.txt", "line1\nmodified line2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: to_non_root_path("file.txt")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: to_non_root_path("file.txt")?,
            replacement_path: None,
        });

        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: true,
        };

        let diff = unified(
            &ctx,
            &repo_ctx,
            Some(base_input),
            Some(other_input),
            options,
        )
        .await?;

        // When omit = true, xdiff assumes that we don't want to display the content because they
        // are binaries.
        let expected = "diff --git a/file.txt b/file.txt\nBinary file file.txt has changed\n";
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert_eq!(diff_str, expected);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_unified_with_none_inputs(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo_ctx = init_test_repo(&ctx).await?;

        // Create a test commit with content
        let cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .add_file("file.txt", "some content\nline2\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: cs,
            path: to_non_root_path("file.txt")?,
            replacement_path: None,
        });

        let options = UnifiedDiffOpts {
            context: 3,
            copy_info: DiffCopyInfo::None,
            file_type: DiffFileType::Regular,
            inspect_lfs_pointers: true,
            omit_content: false,
        };

        // Test None vs Some - should show addition
        let diff = unified(&ctx, &repo_ctx, None, Some(input.clone()), options.clone()).await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(diff_str.contains("+some content"));
        assert!(diff_str.contains("+line2"));
        assert!(!diff.is_binary);

        // Test Some vs None - should show deletion
        let diff = unified(&ctx, &repo_ctx, Some(input), None, options.clone()).await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(diff_str.contains("-some content"));
        assert!(diff_str.contains("-line2"));
        assert!(!diff.is_binary);

        // Test None vs None - should show no difference
        let diff = unified(&ctx, &repo_ctx, None, None, options).await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        // Empty diff between two empty files
        assert!(diff_str.is_empty() || diff_str.trim().is_empty());
        assert!(!diff.is_binary);

        Ok(())
    }
}
