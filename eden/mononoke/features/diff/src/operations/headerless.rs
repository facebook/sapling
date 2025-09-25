/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::anyhow;
use bytes::Bytes;
use context::CoreContext;
use futures::try_join;
use mononoke_api::MononokeRepo;
use mononoke_api::RepoContext;

use crate::types::DiffSingleInput;
use crate::types::HeaderlessDiffOpts;
use crate::types::HeaderlessUnifiedDiff;
use crate::utils::content::load_content;

pub async fn headerless_unified<R: MononokeRepo>(
    ctx: &CoreContext,
    repo: &RepoContext<R>,
    base: Option<DiffSingleInput>,
    other: Option<DiffSingleInput>,
    context: usize,
) -> Result<HeaderlessUnifiedDiff, Error> {
    let (base_bytes, other_bytes) = try_join!(
        async {
            if let Some(base_input) = &base {
                load_content(ctx, repo, base_input.clone()).await
            } else {
                Ok(None)
            }
        },
        async {
            if let Some(other_input) = &other {
                load_content(ctx, repo, other_input.clone()).await
            } else {
                Ok(None)
            }
        }
    )?;

    let (base_content, other_content) = match (base_bytes, other_bytes) {
        (None, None) => return Err(anyhow!("All inputs to the headerless diff were empty")),
        (Some(base), None) => (base, Bytes::new()),
        (None, Some(other)) => (Bytes::new(), other),
        (Some(base), Some(other)) => (base, other),
    };

    let is_binary = xdiff::file_is_binary(&Some(xdiff::DiffFile {
        path: "base".to_string(),
        contents: xdiff::FileContent::Inline(base_content.clone()),
        file_type: xdiff::FileType::Regular,
    })) || xdiff::file_is_binary(&Some(xdiff::DiffFile {
        path: "other".to_string(),
        contents: xdiff::FileContent::Inline(other_content.clone()),
        file_type: xdiff::FileType::Regular,
    }));

    let opts = HeaderlessDiffOpts { context };
    let xdiff_opts = xdiff::HeaderlessDiffOpts::from(opts);

    let raw_diff = if is_binary {
        b"Binary files differ\n".to_vec()
    } else {
        xdiff::diff_unified_headerless(&other_content, &base_content, xdiff_opts)
    };

    Ok(HeaderlessUnifiedDiff {
        raw_diff,
        is_binary,
    })
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
    use crate::types::DiffInputChangesetPath;
    use crate::types::DiffSingleInput;

    async fn init_test_repo(ctx: &CoreContext) -> Result<RepoContext<Repo>, Error> {
        let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
        let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
        Ok(repo_ctx)
    }

    fn create_non_root_path(path: &str) -> Result<mononoke_types::NonRootMPath, Error> {
        let mpath = mononoke_types::MPath::new(path)?;
        let non_root_mpath = mononoke_types::NonRootMPath::try_from(mpath)?;
        Ok(non_root_mpath)
    }

    #[mononoke::fbinit_test]
    async fn test_headerless_unified_basic(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo_ctx = init_test_repo(&ctx).await?;

        // Create test commits with different content
        let base_cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .add_file("file.txt", "line1\nline2\nline3\n")
            .commit()
            .await?;

        let other_cs = CreateCommitContext::new(&ctx, repo_ctx.repo(), vec![base_cs])
            .add_file("file.txt", "line1\nmodified line2\nline3\n")
            .commit()
            .await?;

        // Test the headerless diff function
        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: create_non_root_path("file.txt")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: create_non_root_path("file.txt")?,
            replacement_path: None,
        });

        let diff =
            headerless_unified(&ctx, &repo_ctx, Some(base_input), Some(other_input), 3).await?;

        let expected_diff = "@@ -1,3 +1,3 @@\n line1\n-modified line2\n+line2\n line3\n";

        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert_eq!(diff_str, expected_diff);
        assert!(!diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_headerless_unified_binary_files(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo_ctx = init_test_repo(&ctx).await?;

        // Create test commits with binary content (contains null bytes)
        let base_cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .add_file("binary.bin", b"binary\x00content\x01here".as_slice())
            .commit()
            .await?;

        let other_cs = CreateCommitContext::new(&ctx, repo_ctx.repo(), vec![base_cs])
            .add_file("binary.bin", b"different\x00binary\x02data".as_slice())
            .commit()
            .await?;

        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: create_non_root_path("binary.bin")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: create_non_root_path("binary.bin")?,
            replacement_path: None,
        });

        let diff =
            headerless_unified(&ctx, &repo_ctx, Some(base_input), Some(other_input), 3).await?;

        let expected_diff = "Binary files differ\n";
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert_eq!(diff_str, expected_diff);
        assert!(diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_headerless_unified_empty_files(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo_ctx = init_test_repo(&ctx).await?;

        // Test with one empty file and one with content
        let base_cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .commit()
            .await?;

        let other_cs = CreateCommitContext::new(&ctx, repo_ctx.repo(), vec![base_cs])
            .add_file("new_file.txt", "new content\nline2\n")
            .commit()
            .await?;

        let base_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: base_cs,
            path: create_non_root_path("new_file.txt")?,
            replacement_path: None,
        });
        let other_input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: other_cs,
            path: create_non_root_path("new_file.txt")?,
            replacement_path: None,
        });

        let diff =
            headerless_unified(&ctx, &repo_ctx, Some(base_input), Some(other_input), 3).await?;

        let expected_diff = "@@ -1,2 +0,0 @@\n-new content\n-line2\n";
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert_eq!(diff_str, expected_diff);
        assert!(!diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_headerless_unified_with_none_inputs(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo_ctx = init_test_repo(&ctx).await?;

        // Create a test commit with content
        let cs = CreateCommitContext::new_root(&ctx, repo_ctx.repo())
            .add_file("file.txt", "some content\nline2\n")
            .commit()
            .await?;

        let input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: cs,
            path: create_non_root_path("file.txt")?,
            replacement_path: None,
        });

        // Test None vs Some - should show addition
        let diff = headerless_unified(&ctx, &repo_ctx, None, Some(input.clone()), 3).await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(!diff.is_binary);
        assert_eq!(diff_str, "@@ -1,2 +0,0 @@\n-some content\n-line2\n");

        // Test Some vs None - should show deletion
        let diff = headerless_unified(&ctx, &repo_ctx, Some(input), None, 3).await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(!diff.is_binary);
        assert_eq!(diff_str, "@@ -0,0 +1,2 @@\n+some content\n+line2\n");

        // Test None vs None - should return an error
        let result = headerless_unified(&ctx, &repo_ctx, None, None, 3).await;
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert_eq!(
            error_message,
            "All inputs to the headerless diff were empty"
        );

        Ok(())
    }
}
