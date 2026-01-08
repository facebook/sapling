/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bytes::Bytes;
use context::CoreContext;
use futures::try_join;

use crate::error::DiffError;
use crate::types::DiffSingleInput;
use crate::types::HeaderlessDiffOpts;
use crate::types::HeaderlessUnifiedDiff;
use crate::types::Repo;
use crate::utils::content::load_content;
use crate::utils::whitespace::strip_horizontal_whitespace;

pub async fn headerless_unified(
    ctx: &CoreContext,
    repo: &impl Repo,
    base: Option<DiffSingleInput>,
    other: Option<DiffSingleInput>,
    options: HeaderlessDiffOpts,
) -> Result<HeaderlessUnifiedDiff, DiffError> {
    let ignore_whitespace = options.ignore_whitespace;

    let (base_bytes, other_bytes) = try_join!(
        async {
            if let Some(base_input) = &base {
                load_content(ctx, repo, base_input).await
            } else {
                Ok(None)
            }
        },
        async {
            if let Some(other_input) = &other {
                load_content(ctx, repo, other_input).await
            } else {
                Ok(None)
            }
        }
    )?;

    let (mut base_content, mut other_content) = match (base_bytes, other_bytes) {
        (None, None) => return Err(DiffError::empty_inputs()),
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

    // Only strip whitespace if NOT binary and ignore_whitespace is enabled
    if !is_binary && ignore_whitespace {
        base_content = strip_horizontal_whitespace(&base_content);
        other_content = strip_horizontal_whitespace(&other_content);
    }

    let xdiff_opts = xdiff::HeaderlessDiffOpts::from(options);

    let raw_diff = if is_binary {
        b"Binary files differ".to_vec()
    } else {
        tokio::task::spawn_blocking(move || {
            xdiff::diff_unified_headerless(&other_content, &base_content, xdiff_opts)
        })
        .await
        .map_err(|e| DiffError::internal(anyhow::anyhow!("spawn_blocking failed: {}", e)))?
    };

    Ok(HeaderlessUnifiedDiff {
        raw_diff,
        is_binary,
    })
}

#[cfg(test)]
mod tests {

    use fbinit::FacebookInit;
    use mononoke_macros::mononoke;
    use test_repo_factory;
    use tests_utils::BasicTestRepo;
    use tests_utils::CreateCommitContext;

    use super::*;
    use crate::types::DiffInputChangesetPath;
    use crate::types::DiffSingleInput;

    async fn init_test_repo(ctx: &CoreContext) -> Result<BasicTestRepo, DiffError> {
        let repo = test_repo_factory::build_empty(ctx.fb)
            .await
            .map_err(DiffError::internal)?;
        Ok(repo)
    }

    fn create_non_root_path(path: &str) -> Result<mononoke_types::NonRootMPath, DiffError> {
        let mpath = mononoke_types::MPath::new(path)?;
        let non_root_mpath = mononoke_types::NonRootMPath::try_from(mpath)?;
        Ok(non_root_mpath)
    }

    #[mononoke::fbinit_test]
    async fn test_headerless_unified_basic(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create test commits with different content
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "line1\nline2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("file.txt", "line1\nmodified line2\nline3\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

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

        let options = HeaderlessDiffOpts {
            context: 3,
            ignore_whitespace: false,
        };
        let diff =
            headerless_unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;

        let expected_diff = "@@ -1,3 +1,3 @@\n line1\n-modified line2\n+line2\n line3\n";

        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert_eq!(diff_str, expected_diff);
        assert!(!diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_headerless_unified_binary_files(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create test commits with binary content (contains null bytes)
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("binary.bin", b"binary\x00content\x01here".as_slice())
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("binary.bin", b"different\x00binary\x02data".as_slice())
            .commit()
            .await
            .map_err(DiffError::internal)?;

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

        let options = HeaderlessDiffOpts {
            context: 3,
            ignore_whitespace: false,
        };
        let diff =
            headerless_unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;

        let expected_diff = "Binary files differ";
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert_eq!(diff_str, expected_diff);
        assert!(diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_headerless_unified_empty_files(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Test with one empty file and one with content
        let base_cs = CreateCommitContext::new_root(&ctx, &repo)
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let other_cs = CreateCommitContext::new(&ctx, &repo, vec![base_cs])
            .add_file("new_file.txt", "new content\nline2\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

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

        let options = HeaderlessDiffOpts {
            context: 3,
            ignore_whitespace: false,
        };
        let diff =
            headerless_unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;

        let expected_diff = "@@ -1,2 +0,0 @@\n-new content\n-line2\n";
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert_eq!(diff_str, expected_diff);
        assert!(!diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_headerless_unified_with_none_inputs(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        // Create a test commit with content
        let cs = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file.txt", "some content\nline2\n")
            .commit()
            .await
            .map_err(DiffError::internal)?;

        let input = DiffSingleInput::ChangesetPath(DiffInputChangesetPath {
            changeset_id: cs,
            path: create_non_root_path("file.txt")?,
            replacement_path: None,
        });

        // Test None vs Some - Should show deletion
        let options = HeaderlessDiffOpts {
            context: 3,
            ignore_whitespace: false,
        };
        let diff =
            headerless_unified(&ctx, &repo, None, Some(input.clone()), options.clone()).await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(!diff.is_binary);
        assert_eq!(diff_str, "@@ -1,2 +0,0 @@\n-some content\n-line2\n");

        // Test Some vs None - Should show addition
        let diff = headerless_unified(&ctx, &repo, Some(input), None, options.clone()).await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(!diff.is_binary);
        assert_eq!(diff_str, "@@ -0,0 +1,2 @@\n+some content\n+line2\n");

        // Test None vs None - should return an error
        let result = headerless_unified(&ctx, &repo, None, None, options).await;
        assert!(result.is_err());
        let error_message = result.unwrap_err().to_string();
        assert_eq!(
            error_message,
            "All inputs to the headerless diff were empty"
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_headerless_unified_string_inputs(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        // Test with String inputs
        let base_input = DiffSingleInput::String(DiffInputString {
            content: "line1\nline2\nline3\n".to_string(),
        });
        let other_input = DiffSingleInput::String(DiffInputString {
            content: "line1\nmodified line2\nline3\n".to_string(),
        });

        let options = HeaderlessDiffOpts {
            context: 3,
            ignore_whitespace: false,
        };
        let diff =
            headerless_unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;

        let expected_diff = "@@ -1,3 +1,3 @@\n line1\n-modified line2\n+line2\n line3\n";

        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert_eq!(diff_str, expected_diff);
        assert!(!diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_headerless_unified_string_vs_none(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        let string_input = DiffSingleInput::String(DiffInputString {
            content: "some content\nline2\n".to_string(),
        });

        // Test None vs String - Should show deletion
        let options = HeaderlessDiffOpts {
            context: 3,
            ignore_whitespace: false,
        };
        let diff = headerless_unified(
            &ctx,
            &repo,
            None,
            Some(string_input.clone()),
            options.clone(),
        )
        .await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(!diff.is_binary);
        assert_eq!(diff_str, "@@ -1,2 +0,0 @@\n-some content\n-line2\n");

        // Test String vs None - Should show addition
        let diff = headerless_unified(&ctx, &repo, Some(string_input), None, options).await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert!(!diff.is_binary);
        assert_eq!(diff_str, "@@ -0,0 +1,2 @@\n+some content\n+line2\n");

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_headerless_unified_string_binary(fb: FacebookInit) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        // Test with binary String inputs (contains null bytes)
        let base_input = DiffSingleInput::String(DiffInputString {
            content: String::from_utf8_lossy(b"binary\x00content").to_string(),
        });
        let other_input = DiffSingleInput::String(DiffInputString {
            content: String::from_utf8_lossy(b"different\x00binary").to_string(),
        });

        let options = HeaderlessDiffOpts {
            context: 3,
            ignore_whitespace: false,
        };
        let diff =
            headerless_unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;

        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        assert_eq!(diff_str, "Binary files differ");
        assert!(diff.is_binary);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_headerless_unified_ignore_whitespace_only(
        fb: FacebookInit,
    ) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        // Test with only whitespace differences
        let base_input = DiffSingleInput::String(DiffInputString {
            content: "hello world\nfoo bar\n".to_string(),
        });
        let other_input = DiffSingleInput::String(DiffInputString {
            content: "hello  world\nfoo\tbar\n".to_string(),
        });

        // With ignore_whitespace: false, should show differences
        let options = HeaderlessDiffOpts {
            context: 3,
            ignore_whitespace: false,
        };
        let diff = headerless_unified(
            &ctx,
            &repo,
            Some(base_input.clone()),
            Some(other_input.clone()),
            options,
        )
        .await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        // Should show some differences (either + or - lines)
        assert!(
            !diff_str.is_empty(),
            "Diff should show whitespace differences when ignore_whitespace=false"
        );

        // With ignore_whitespace: true, should show no/minimal differences
        // (After stripping whitespace, "helloworld\nfoobar\n" should match on both sides)
        let options = HeaderlessDiffOpts {
            context: 3,
            ignore_whitespace: true,
        };
        let diff =
            headerless_unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        // After stripping whitespace, content should be identical
        assert!(
            diff_str.is_empty(),
            "Diff should show no changes when ignore_whitespace=true for whitespace-only changes"
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_headerless_unified_ignore_whitespace_mixed(
        fb: FacebookInit,
    ) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        // Test with both whitespace and content differences
        let base_input = DiffSingleInput::String(DiffInputString {
            content: "line1\nline2\nline3\n".to_string(),
        });
        let other_input = DiffSingleInput::String(DiffInputString {
            content: "line1\nmodified  line2\nline3\n".to_string(),
        });

        // With ignore_whitespace: true, should still show content change
        // After stripping whitespace: "line2" vs "modifiedline2" (real difference!)
        let options = HeaderlessDiffOpts {
            context: 3,
            ignore_whitespace: true,
        };
        let diff =
            headerless_unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;
        let diff_str = String::from_utf8_lossy(&diff.raw_diff);
        // Should show a diff because there's actual content difference beyond whitespace
        assert!(
            !diff_str.is_empty() && diff_str.contains("@@"),
            "Diff should show content changes even with ignore_whitespace=true"
        );

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_headerless_unified_ignore_whitespace_binary(
        fb: FacebookInit,
    ) -> Result<(), DiffError> {
        let ctx = CoreContext::test_mock(fb);
        let repo = init_test_repo(&ctx).await?;

        use crate::types::DiffInputString;

        // Test that binary files are not affected by whitespace stripping
        let base_input = DiffSingleInput::String(DiffInputString {
            content: String::from_utf8_lossy(b"binary\x00 content").to_string(),
        });
        let other_input = DiffSingleInput::String(DiffInputString {
            content: String::from_utf8_lossy(b"binary\x00  content").to_string(),
        });

        let options = HeaderlessDiffOpts {
            context: 3,
            ignore_whitespace: true,
        };
        let diff =
            headerless_unified(&ctx, &repo, Some(base_input), Some(other_input), options).await?;

        // Should be detected as binary
        assert!(diff.is_binary);
        assert_eq!(
            String::from_utf8_lossy(&diff.raw_diff),
            "Binary files differ"
        );

        Ok(())
    }
}
