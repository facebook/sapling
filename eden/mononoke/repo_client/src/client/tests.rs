/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg(test)]

use fbinit::FacebookInit;
use mercurial_derivation::DeriveHgChangeset;
use mononoke_macros::mononoke;
use mononoke_types_mocks::changesetid::ONES_CSID;
use serde_json::json;
use tests_utils::CreateCommitContext;

use super::*;
use crate::repo::RepoClientRepo;

#[mononoke::fbinit_test]
async fn test_maybe_validate_pushed_bonsais(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo: RepoClientRepo = test_repo_factory::build_empty(ctx.fb).await?;
    let commit = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("largefile", "11111_11111")
        .commit()
        .await?;

    let hg_cs_id = repo.derive_hg_changeset(&ctx, commit).await?;

    // No replay data - ignore
    maybe_validate_pushed_bonsais(&ctx, &repo, &None).await?;

    // Has replay data, but no hgbonsaimapping - ignore
    maybe_validate_pushed_bonsais(&ctx, &repo, &Some("{}".to_string())).await?;

    // Has valid replay data - should succeed
    maybe_validate_pushed_bonsais(
        &ctx,
        &repo,
        &Some(
            json!({
                "hgbonsaimapping": {
                    format!("{}", hg_cs_id): commit,
                }
            })
            .to_string(),
        ),
    )
    .await?;

    // Additional fields doesn't change the result
    maybe_validate_pushed_bonsais(
        &ctx,
        &repo,
        &Some(
            json!({
                "hgbonsaimapping": {
                    format!("{}", hg_cs_id): commit,
                },
                "somefield": "somevalue"
            })
            .to_string(),
        ),
    )
    .await?;

    // Now invalid bonsai - should fail
    assert!(
        maybe_validate_pushed_bonsais(
            &ctx,
            &repo,
            &Some(
                json!({
                    "hgbonsaimapping": {
                        format!("{}", hg_cs_id): ONES_CSID,
                    },
                    "somefield": "somevalue"
                })
                .to_string(),
            ),
        )
        .await
        .is_err()
    );

    // Now invalid hgbonsaimapping field - should fail
    assert!(
        maybe_validate_pushed_bonsais(
            &ctx,
            &repo,
            &Some(
                json!({
                    "hgbonsaimapping": "somevalue"
                })
                .to_string(),
            ),
        )
        .await
        .is_err()
    );
    Ok(())
}

#[mononoke::test]
fn test_parse_git_lookup() -> Result<(), Error> {
    assert!(parse_git_lookup("ololo").is_none());
    assert!(parse_git_lookup("_gitlookup_hg_badhash").is_none());
    assert!(parse_git_lookup("_gitlookup_git_badhash").is_none());
    assert_eq!(
        parse_git_lookup("_gitlookup_hg_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        Some(GitLookup::HgToGit(HgChangesetId::from_str(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        )?))
    );

    assert_eq!(
        parse_git_lookup("_gitlookup_git_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"),
        Some(GitLookup::GitToHg(GitSha1::from_str(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        )?))
    );

    Ok(())
}
