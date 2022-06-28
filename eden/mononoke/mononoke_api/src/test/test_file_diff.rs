/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use fbinit::FacebookInit;
use tests_utils::drawdag::changes;
use tests_utils::drawdag::create_from_dag_with_changes;

use crate::headerless_unified_diff;
use crate::ChangesetId;
use crate::CoreContext;
use crate::Repo;
use crate::RepoContext;

async fn init_repo(ctx: &CoreContext) -> Result<(RepoContext, BTreeMap<String, ChangesetId>)> {
    let blob_repo = test_repo_factory::build_empty(ctx.fb)?;
    let changesets = create_from_dag_with_changes(
        ctx,
        &blob_repo,
        r##"
            A-B-C
        "##,
        changes! {
            "B" => |c| c.add_file("file", "test\nbefore\ndata\n").add_file("bin", "bin\0\x01"),
            "C" => |c| c.add_file("file", "test\nafter\ndata\n").add_file("bin", "bin\0\x02"),
        },
    )
    .await?;

    let repo = Repo::new_test(ctx.clone(), blob_repo).await?;
    let repo_ctx = RepoContext::new_test(ctx.clone(), Arc::new(repo)).await?;
    Ok((repo_ctx, changesets))
}

#[fbinit::test]
async fn file_diff(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let (repo, changesets) = init_repo(&ctx).await?;

    let b = repo
        .changeset(changesets["B"])
        .await?
        .expect("changeset should exist");
    let c = repo
        .changeset(changesets["C"])
        .await?
        .expect("changeset should exist");

    // Compare two regular files
    let b_file = b
        .path_with_content("file")?
        .file()
        .await?
        .expect("should be a file");
    let c_file = c
        .path_with_content("file")?
        .file()
        .await?
        .expect("should be a file");
    let diff = headerless_unified_diff(&b_file, &c_file, 3).await?;
    assert_eq!(
        std::str::from_utf8(&diff.raw_diff)?,
        concat!(
            "@@ -1,3 +1,3 @@\n",
            " test\n",
            "-before\n",
            "+after\n",
            " data\n"
        )
    );
    assert!(!diff.is_binary);

    // Compare two binary files
    let b_bin = b
        .path_with_content("bin")?
        .file()
        .await?
        .expect("should be a file");
    let c_bin = c
        .path_with_content("bin")?
        .file()
        .await?
        .expect("should be a file");
    let diff = headerless_unified_diff(&b_bin, &c_bin, 3).await?;
    assert_eq!(std::str::from_utf8(&diff.raw_diff)?, "Binary files differ");
    assert!(diff.is_binary);
    Ok(())
}
