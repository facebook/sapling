/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;

use anyhow::Error;
use context::CoreContext;
use mercurial_types::NonRootMPath;
use mononoke_types::ChangesetId;

use crate::chunking::Chunker;
use crate::common::delete_files_in_chunks;
use crate::common::ChangesetArgsFactory;
use crate::working_copy::get_changed_content_working_copy_paths;
use crate::Repo;

/// A struct containing pre-merge delete information
/// Pre-merge delete commits look like this:
/// ```text
///                         C3'
///                         |
///       D2                C2'
///       |                 |
///       D1                C1'
///       |                 |
///       master         correct
/// ```
/// Where:
///   `D1`, `D2`: are gradual deletion commits of files that need their history fixed up
///   `C1`, `C2`, `C3`: are gradual deletion commits of all of other files on the branch
///                     containing correct history (so we merge only the files that needed fixup)
///   `master`: a head of an independent DAG to be merged into
///   `master`: a head of an independent DAG to be merged into
///   `correct`: branch containing correct history for paths
///
/// Note that the order of commits in `delete_commits_fixup_branch`
/// corresponds to the order of indices on the diagram:
/// - `delete_commits_fixup_branch = [D1, D2]`
pub struct HistoryFixupDeletes {
    pub delete_commits_fixup_branch: Vec<ChangesetId>,
    pub delete_commits_correct_branch: Vec<ChangesetId>,
}

/// Create `HistoryFixupDeletes` struct, implementing gradual delete strategy
/// See the struct's docstring for more details about the end state
/// See also <https://fb.quip.com/JfHhAyOZ2FBj> for strategy and discussion
pub async fn create_history_fixup_deletes<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    fixup_bcs_id: ChangesetId,
    chunker: Chunker<NonRootMPath>,
    delete_commits_changeset_args_factory: impl ChangesetArgsFactory,
    correct_bcs_id: ChangesetId,
    paths_to_fixup: Vec<NonRootMPath>,
) -> Result<HistoryFixupDeletes, Error> {
    let delete_commits_fixup_branch = delete_files_in_chunks(
        ctx,
        repo,
        fixup_bcs_id,
        paths_to_fixup.clone(),
        &chunker,
        &delete_commits_changeset_args_factory,
        false, /* skip last chunk */
    )
    .await?;

    let mut paths_to_remove: BTreeSet<_> =
        get_changed_content_working_copy_paths(ctx, repo, correct_bcs_id, fixup_bcs_id)
            .await?
            .into_iter()
            .collect();
    for path in paths_to_fixup.iter() {
        paths_to_remove.remove(path);
    }
    let delete_commits_correct_branch = delete_files_in_chunks(
        ctx,
        repo,
        correct_bcs_id,
        paths_to_remove.into_iter().collect(),
        &chunker,
        &delete_commits_changeset_args_factory,
        false, /* skip last chunk */
    )
    .await?;

    Ok(HistoryFixupDeletes {
        delete_commits_fixup_branch,
        delete_commits_correct_branch,
    })
}

#[cfg(test)]
mod test {
    use std::collections::BTreeSet;

    use bonsai_hg_mapping::BonsaiHgMapping;
    use bookmarks::Bookmarks;
    use cloned::cloned;
    use commit_graph::CommitGraph;
    use commit_graph::CommitGraphWriter;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use fixtures::TestRepoFixture;
    use fixtures::UnsharedMergeUneven;
    use mononoke_macros::mononoke;
    use mononoke_types::DateTime;
    use phases::Phases;
    use repo_blobstore::RepoBlobstore;
    use repo_derived_data::RepoDerivedData;
    use repo_identity::RepoIdentity;
    use tests_utils::resolve_cs_id;

    use super::*;
    use crate::common::ChangesetArgs;
    use crate::common::StackPosition;
    use crate::working_copy::get_working_copy_paths;

    #[facet::container]
    #[derive(Clone)]
    struct TestRepo(
        dyn BonsaiHgMapping,
        dyn Bookmarks,
        RepoBlobstore,
        RepoDerivedData,
        RepoIdentity,
        CommitGraph,
        dyn CommitGraphWriter,
        FilestoreConfig,
        dyn Phases,
    );

    #[mononoke::fbinit_test]
    async fn test_create_fixup_deletes(fb: FacebookInit) -> Result<(), Error> {
        let repo: TestRepo = UnsharedMergeUneven::get_repo(fb).await;
        let ctx = CoreContext::test_mock(fb);

        // Side of the history that needs fixing up is one line of commit with the following
        // files in the working copy: 1 2 3 4 5
        // The "correct" history has: 1 2 3 4 5 6 7
        // This test attempts to fixup the history of files 1 and 2.
        let fixup_bcs_id =
            resolve_cs_id(&ctx, &repo, "03b0589d9788870817d03ce7b87516648ed5b33a").await?;
        let correct_bcs_id =
            resolve_cs_id(&ctx, &repo, "5a3e8d5a475ec07895e64ec1e1b2ec09bfa70e4e").await?;
        let create_delete_cs_args = |num: StackPosition| ChangesetArgs {
            author: "user".to_string(),
            message: format!("Delete: {}", num.0),
            datetime: DateTime::from_rfc3339("1985-04-12T23:20:50.52Z").unwrap(),
            bookmark: None,
            mark_public: false,
        };

        let one = NonRootMPath::new("1").unwrap();
        let two = NonRootMPath::new("2").unwrap();
        let five = NonRootMPath::new("5").unwrap();
        let six = NonRootMPath::new("6").unwrap();

        // Arrange everything into [[1], [...], [10]]
        let chunker = Box::new({
            cloned!(one);
            move |mpaths| {
                let mut v1: Vec<NonRootMPath> = vec![];
                let mut v2: Vec<NonRootMPath> = vec![];

                for mpath in mpaths {
                    if mpath == one {
                        v1.push(mpath);
                    } else {
                        v2.push(mpath);
                    }
                }

                if v1.is_empty() {
                    vec![v2]
                } else if v2.is_empty() {
                    vec![v1]
                } else {
                    vec![v1, v2]
                }
            }
        });

        let hfd = create_history_fixup_deletes(
            &ctx,
            &repo,
            fixup_bcs_id,
            chunker,
            create_delete_cs_args,
            correct_bcs_id,
            vec![one.clone(), two.clone()],
        )
        .await?;

        let HistoryFixupDeletes {
            delete_commits_fixup_branch,
            delete_commits_correct_branch,
        } = hfd;

        assert_eq!(delete_commits_fixup_branch.len(), 2);
        assert_eq!(delete_commits_correct_branch.len(), 1);

        // Validate delete commits
        let fixup_branch_after_deletions = delete_commits_fixup_branch[1];
        let correct_branch_after_deletions = delete_commits_correct_branch[0];

        // We expect that the "fixup" branch which used to have files 1-5 to
        // have just files 3-5 (no more 1 and 2)
        let fixup_working_copy: BTreeSet<NonRootMPath> =
            get_working_copy_paths(&ctx, &repo, fixup_branch_after_deletions)
                .await
                .unwrap()
                .into_iter()
                .collect();

        assert!(!fixup_working_copy.contains(&one));
        assert!(!fixup_working_copy.contains(&two));
        assert!(fixup_working_copy.contains(&five));
        assert!(!fixup_working_copy.contains(&six));

        // We expect that the "correct" branch which used to have files 1-7 to
        // have just files 1-5 (because we want to merge in 1-2 and 3-5 are the same
        // so they don't matter, 6-7 are not present in the fixup branch).
        let correct_working_copy: BTreeSet<NonRootMPath> =
            get_working_copy_paths(&ctx, &repo, correct_branch_after_deletions)
                .await
                .unwrap()
                .into_iter()
                .collect();

        assert!(correct_working_copy.contains(&one));
        assert!(correct_working_copy.contains(&two));
        assert!(correct_working_copy.contains(&five));
        assert!(!correct_working_copy.contains(&six));
        Ok(())
    }
}
