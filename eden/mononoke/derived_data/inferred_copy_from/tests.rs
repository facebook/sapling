/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use blobstore::Loadable;
use context::CoreContext;
use facet::futures::TryStreamExt;
use fbinit::FacebookInit;
use mononoke_api::repo::Repo;
use mononoke_api::repo::RepoContext;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::MPath;
use mononoke_types::inferred_copy_from::InferredCopyFromEntry;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use tests_utils::CreateCommitContext;

use super::*;

async fn init_repo(ctx: &CoreContext) -> Result<(Repo, HashMap<&'static str, ChangesetId>)> {
    let repo: Repo = test_repo_factory::build_empty(ctx.fb).await?;
    let mut changesets = HashMap::new();

    changesets.insert(
        "a",
        CreateCommitContext::new_root(ctx, &repo)
            .add_file("path/to/file1", "abc\n")
            .set_author_date(DateTime::from_timestamp(1000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "aa",
        CreateCommitContext::new_root(ctx, &repo)
            .add_file("path/to/file2", "def\n")
            .set_author_date(DateTime::from_timestamp(1000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "b",
        CreateCommitContext::new(ctx, &repo, vec![changesets["a"]])
            .add_file("path/to/file3", "ghi\n")
            .set_author_date(DateTime::from_timestamp(1000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "c",
        CreateCommitContext::new(ctx, &repo, vec![changesets["aa"], changesets["b"]])
            // Inferred renames:
            // b:path/to/file1 -> new/path/to/file1
            // b:path/to/file1 -> another/new/path/to/file1
            // aa:path/to/file2 -> new/path/to/file2
            .add_file("new/path/to/file1", "abc\n")
            .add_file("another/new/path/to/file1", "abc\n")
            .add_file("new/path/to/file2", "def\n")
            .delete_file("path/to/file1")
            .delete_file("path/to/file2")
            .set_author_date(DateTime::from_timestamp(1000, 0)?)
            .commit()
            .await?,
    );

    changesets.insert(
        "d",
        CreateCommitContext::new(ctx, &repo, vec![changesets["c"]])
            .add_file("path/to/basename1", "aabbcc\n")
            .add_file("path/to/basename2", "ddeeff\n")
            .set_author_date(DateTime::from_timestamp(1000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "e",
        CreateCommitContext::new(ctx, &repo, vec![changesets["d"]])
            // Inferred copies:
            // d:path/to/basename1 -> path/basename1
            // d:path/to/basename2 -> path/basename2
            // d:path/to/basename2 -> another/path/basename2
            .add_file("path/basename1", "aabbcc\n")
            .add_file("path/basename2", "ddeeff\n")
            .add_file("another/path/basename2", "ddeeff\n")
            .set_author_date(DateTime::from_timestamp(1000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "f",
        CreateCommitContext::new(ctx, &repo, vec![changesets["d"]])
            // Not detected due to the directory constraint.
            .add_file("another/path/basename2", "ddeeff\n")
            .set_author_date(DateTime::from_timestamp(1000, 0)?)
            .commit()
            .await?,
    );

    changesets.insert(
        "g",
        CreateCommitContext::new(ctx, &repo, vec![changesets["f"]])
            .add_file("test/file1", "hello\nworld\n")
            .add_file("test/file2", "one\ntwo\nthree\n")
            .set_author_date(DateTime::from_timestamp(1000, 0)?)
            .commit()
            .await?,
    );
    changesets.insert(
        "h",
        CreateCommitContext::new(ctx, &repo, vec![changesets["g"]])
            // Rename with modification
            // g:test/file1 -> test/partial/match/file1
            .add_file("test/partial/match/file1", "hello\nworld!\n")
            .delete_file("test/file1")
            // Copy with modification
            // g:test/file2 -> test/partial/match/file2
            .add_file("test/partial/match/file2", "one\ntwo\nfour\n")
            // Non-match due to content being too different
            .add_file("test/another/file2", "one\ntwo\nthree\nfour\nfive\nsix\n")
            // Modified an existing file
            .add_file("test/file2", "one\ntwo\n")
            .set_author_date(DateTime::from_timestamp(1000, 0)?)
            .commit()
            .await?,
    );

    Ok((repo, changesets))
}

#[mononoke::fbinit_test]
async fn derive_single_test(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;
    let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo.clone())).await?;

    assert_entries(
        &ctx,
        &repo,
        repo_ctx.changeset(changesets["a"]).await?.unwrap().id(),
        &[],
    )
    .await?;

    assert_entries(
        &ctx,
        &repo,
        repo_ctx.changeset(changesets["b"]).await?.unwrap().id(),
        &[],
    )
    .await?;

    assert_entries(
        &ctx,
        &repo,
        repo_ctx.changeset(changesets["c"]).await?.unwrap().id(),
        &[
            (
                MPath::new("another/new/path/to/file1")?,
                InferredCopyFromEntry {
                    from_csid: changesets["b"],
                    from_path: MPath::new("path/to/file1")?,
                },
            ),
            (
                MPath::new("new/path/to/file1")?,
                InferredCopyFromEntry {
                    from_csid: changesets["b"],
                    from_path: MPath::new("path/to/file1")?,
                },
            ),
            (
                MPath::new("new/path/to/file2")?,
                InferredCopyFromEntry {
                    from_csid: changesets["aa"],
                    from_path: MPath::new("path/to/file2")?,
                },
            ),
        ],
    )
    .await?;

    assert_entries(
        &ctx,
        &repo,
        repo_ctx.changeset(changesets["e"]).await?.unwrap().id(),
        &[
            (
                MPath::new("another/path/basename2")?,
                InferredCopyFromEntry {
                    from_csid: changesets["d"],
                    from_path: MPath::new("path/to/basename2")?,
                },
            ),
            (
                MPath::new("path/basename1")?,
                InferredCopyFromEntry {
                    from_csid: changesets["d"],
                    from_path: MPath::new("path/to/basename1")?,
                },
            ),
            (
                MPath::new("path/basename2")?,
                InferredCopyFromEntry {
                    from_csid: changesets["d"],
                    from_path: MPath::new("path/to/basename2")?,
                },
            ),
        ],
    )
    .await?;

    assert_entries(
        &ctx,
        &repo,
        repo_ctx.changeset(changesets["f"]).await?.unwrap().id(),
        &[],
    )
    .await?;

    assert_entries(
        &ctx,
        &repo,
        repo_ctx.changeset(changesets["h"]).await?.unwrap().id(),
        &[
            (
                MPath::new("test/partial/match/file1")?,
                InferredCopyFromEntry {
                    from_csid: changesets["g"],
                    from_path: MPath::new("test/file1")?,
                },
            ),
            (
                MPath::new("test/partial/match/file2")?,
                InferredCopyFromEntry {
                    from_csid: changesets["g"],
                    from_path: MPath::new("test/file2")?,
                },
            ),
        ],
    )
    .await?;

    Ok(())
}

async fn assert_entries(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    expected: &[(MPath, InferredCopyFromEntry)],
) -> Result<()> {
    let root_inferred_copy_from_id = repo
        .repo_derived_data()
        .derive::<RootInferredCopyFromId>(ctx, cs_id)
        .await?;
    let inferred_copy_from = root_inferred_copy_from_id
        .into_inner_id()
        .load(ctx, repo.repo_blobstore())
        .await?;
    let entries: Vec<(MPath, InferredCopyFromEntry)> = inferred_copy_from
        .into_subentries(ctx, repo.repo_blobstore())
        .try_collect()
        .await?;

    assert_eq!(entries, expected);

    Ok(())
}
