/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use context::CoreContext;
use mercurial_types::MPath;
use mononoke_types::ChangesetId;

use crate::chunking::Chunker;
use crate::common::delete_files_in_chunks;
use crate::common::ChangesetArgsFactory;
use crate::working_copy::get_changed_working_copy_paths;
use crate::working_copy::get_working_copy_paths;

/// A struct containing pre-merge delete information
/// Pre-merge delete commits look like this:
/// ```text
///       D3
///       |
///       D2
///       |
///       D1
///       |
/// pre-merge-bookmark
/// ```
/// Where:
///   `D1`, `D2`: are gradual deletion commits
///   `pre-merge-bookmark`: a head of an independent DAG to be merged
///
/// Note that the order of commits in `delete_commits`
/// corresponds to the order of indices on the diagram:
/// - `delete_commits = [D1, D3, D3]`
pub struct PreMergeDelete {
    pub delete_commits: Vec<ChangesetId>,
}

/// Create `PreMergeDelete` struct, implementing gradual delete strategy
/// See the struct's docstring for more details about the end state
/// See also <https://fb.quip.com/jPbqA3kK3qCi> for strategy and discussion
pub async fn create_pre_merge_delete<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    parent_bcs_id: ChangesetId,
    chunker: Chunker<MPath>,
    delete_commits_changeset_args_factory: impl ChangesetArgsFactory,
    base_cs_id: Option<ChangesetId>,
) -> Result<PreMergeDelete, Error> {
    let mpaths = match base_cs_id {
        Some(base_cs_id) => {
            get_changed_working_copy_paths(ctx, repo, parent_bcs_id, base_cs_id).await?
        }
        None => get_working_copy_paths(ctx, repo, parent_bcs_id).await?,
    };
    let delete_commits = delete_files_in_chunks(
        ctx,
        repo,
        parent_bcs_id,
        mpaths,
        &chunker,
        &delete_commits_changeset_args_factory,
        true, /* skip_last_chunk */
    )
    .await?;

    Ok(PreMergeDelete { delete_commits })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::common::ChangesetArgs;
    use crate::common::StackPosition;
    use cloned::cloned;
    use fbinit::FacebookInit;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use mononoke_types::DateTime;
    use std::collections::HashSet;
    use tests_utils::resolve_cs_id;
    use tests_utils::CreateCommitContext;

    #[fbinit::test]
    async fn test_create_pre_merge_delete(fb: FacebookInit) -> Result<(), Error> {
        let repo = Linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let bcs_id = resolve_cs_id(&ctx, &repo, "master").await?;
        let create_delete_cs_args = |num: StackPosition| ChangesetArgs {
            author: "user".to_string(),
            message: format!("Delete: {}", num.0),
            datetime: DateTime::from_rfc3339("1985-04-12T23:20:50.52Z").unwrap(),
            bookmark: None,
            mark_public: false,
        };

        let one = MPath::new("1").unwrap();
        let ten = MPath::new("10").unwrap();
        let two = MPath::new("2").unwrap();

        // Arrage everything into [[1], [...], [10]]
        let chunker = Box::new({
            cloned!(one, ten);
            move |mpaths| {
                let mut v1: Vec<MPath> = vec![];
                let mut v2: Vec<MPath> = vec![];
                let mut v3: Vec<MPath> = vec![];

                for mpath in mpaths {
                    if mpath == one {
                        v1.push(mpath);
                    } else if mpath == ten {
                        v3.push(mpath);
                    } else {
                        v2.push(mpath);
                    }
                }

                vec![v1, v2, v3]
            }
        });

        let pmd =
            create_pre_merge_delete(&ctx, &repo, bcs_id, chunker, create_delete_cs_args, None)
                .await?;

        let PreMergeDelete { delete_commits } = pmd;

        assert_eq!(delete_commits.len(), 2);

        // Validate delete commits
        let delete_commit_0 = delete_commits[0];
        let delete_commit_1 = delete_commits[1];

        let working_copy_0: HashSet<MPath> = get_working_copy_paths(&ctx, &repo, delete_commit_0)
            .await
            .unwrap()
            .into_iter()
            .collect();

        assert!(!working_copy_0.contains(&one));
        assert!(working_copy_0.contains(&two));
        assert!(working_copy_0.contains(&ten));

        let working_copy_1: HashSet<MPath> = get_working_copy_paths(&ctx, &repo, delete_commit_1)
            .await
            .unwrap()
            .into_iter()
            .collect();

        assert!(!working_copy_1.contains(&one));
        assert!(!working_copy_1.contains(&two));
        assert!(working_copy_1.contains(&ten));
        Ok(())
    }

    #[fbinit::test]
    async fn test_create_pre_merge_delete_with_base(fb: FacebookInit) -> Result<(), Error> {
        let repo = Linear::getrepo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        let master_bcs_id = resolve_cs_id(&ctx, &repo, "master").await?;
        // Create two commits on top of master:
        //   A   B
        //    \ /
        //     |
        //     O
        //

        let create_delete_cs_args = |num: StackPosition| ChangesetArgs {
            author: "user".to_string(),
            message: format!("Delete: {}", num.0),
            datetime: DateTime::from_rfc3339("1985-04-12T23:20:50.52Z").unwrap(),
            bookmark: None,
            mark_public: false,
        };

        let commit_a = CreateCommitContext::new(&ctx, &repo, vec![master_bcs_id])
            .add_file("common", "common")
            .add_file("changed", "first")
            .add_file("added", "added_content")
            .add_file("added2", "added_content")
            .commit()
            .await?;

        let commit_b = CreateCommitContext::new(&ctx, &repo, vec![master_bcs_id])
            .add_file("common", "common")
            .add_file("changed", "second")
            .add_file("somethingelse", "content")
            .commit()
            .await?;
        let commit_b = CreateCommitContext::new(&ctx, &repo, vec![commit_b])
            .add_file("common", "common")
            // Revert the file content to the same value - it should still be
            // reported as changed
            .add_file("changed", "first")
            .add_file("somethingelse", "content")
            .commit()
            .await?;

        let changed_path = MPath::new("changed")?;
        let added_path = MPath::new("added")?;
        let added2_path = MPath::new("added2")?;

        let chunker = Box::new({
            cloned!(changed_path, added_path);
            move |mpaths| {
                let mut v1: Vec<MPath> = vec![];
                let mut v2: Vec<MPath> = vec![];
                let mut v3: Vec<MPath> = vec![];

                for mpath in mpaths {
                    if mpath == changed_path {
                        v1.push(mpath);
                    } else if mpath == added_path {
                        v2.push(mpath);
                    } else {
                        v3.push(mpath);
                    }
                }

                vec![v1, v2, v3]
            }
        });
        let pmd = create_pre_merge_delete(
            &ctx,
            &repo,
            commit_a,
            chunker,
            create_delete_cs_args,
            Some(commit_b),
        )
        .await?;

        // 2 files should be deleted - "changed" and "added" with two deletion commits
        let PreMergeDelete { delete_commits } = pmd;

        assert_eq!(delete_commits.len(), 2);
        // Validate delete commits
        let delete_commit_0 = delete_commits[0];
        let delete_commit_1 = delete_commits[1];

        let working_copy_0: HashSet<MPath> = get_working_copy_paths(&ctx, &repo, delete_commit_0)
            .await
            .unwrap()
            .into_iter()
            .collect();

        assert!(!working_copy_0.contains(&changed_path));
        assert!(working_copy_0.contains(&added_path));
        assert!(working_copy_0.contains(&added2_path));

        let working_copy_1: HashSet<MPath> = get_working_copy_paths(&ctx, &repo, delete_commit_1)
            .await
            .unwrap()
            .into_iter()
            .collect();

        assert!(!working_copy_1.contains(&changed_path));
        assert!(!working_copy_1.contains(&added_path));
        assert!(working_copy_1.contains(&added2_path));
        Ok(())
    }
}
