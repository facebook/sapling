/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use context::CoreContext;
use derived_data::BonsaiDerived;
use futures::{future::try_join, TryStreamExt};
use manifest::{Diff, ManifestOps};
use mercurial_types::MPath;
use mononoke_types::ChangesetId;
use slog::info;
use std::collections::BTreeMap;
use unodes::RootUnodeManifestId;

use crate::chunking::Chunker;
use crate::common::{create_and_save_bonsai, ChangesetArgsFactory, StackPosition};

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

async fn get_working_copy_paths(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bcs_id: ChangesetId,
) -> Result<Vec<MPath>, Error> {
    let hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
        .await?;

    let hg_cs = hg_cs_id.load(ctx, repo.blobstore()).await?;
    info!(ctx.logger(), "Getting working copy contents");
    let mut paths: Vec<_> = hg_cs
        .manifestid()
        .list_leaf_entries(ctx.clone(), repo.get_blobstore())
        .map_ok(|(path, (_file_type, _filenode_id))| path)
        .try_collect()
        .await?;
    paths.sort();
    info!(ctx.logger(), "Done getting working copy contents");
    Ok(paths)
}

async fn get_changed_working_copy_paths(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bcs_id: ChangesetId,
    base_cs_id: ChangesetId,
) -> Result<Vec<MPath>, Error> {
    let unode_id = RootUnodeManifestId::derive(ctx, repo, bcs_id);
    let base_unode_id = RootUnodeManifestId::derive(ctx, repo, base_cs_id);

    let (unode_id, base_unode_id) = try_join(unode_id, base_unode_id).await?;

    let mut paths = base_unode_id
        .manifest_unode_id()
        .diff(
            ctx.clone(),
            repo.get_blobstore(),
            *unode_id.manifest_unode_id(),
        )
        .try_filter_map(|diff| async move {
            use Diff::*;
            let maybe_path = match diff {
                Added(maybe_path, entry) => entry.into_leaf().and_then(|_| maybe_path),
                Removed(_maybe_path, _entry) => None,
                Changed(maybe_path, _old_entry, new_entry) => {
                    new_entry.into_leaf().and_then(|_| maybe_path)
                }
            };

            Ok(maybe_path)
        })
        .try_collect::<Vec<_>>()
        .await?;

    paths.sort();
    Ok(paths)
}

/// Create `PreMergeDelete` struct, implementing gradual delete strategy
/// See the struct's docstring for more details about the end state
/// See also https://fb.quip.com/jPbqA3kK3qCi for strategy and discussion
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
    info!(ctx.logger(), "Chunking working copy contents");
    let mpath_chunks: Vec<Vec<MPath>> = chunker(mpaths);
    info!(ctx.logger(), "Done chunking working copy contents");

    let mut delete_commits: Vec<ChangesetId> = Vec::new();
    let mut parent = parent_bcs_id;
    let chunk_num = mpath_chunks.len();
    for (i, mpath_chunk) in mpath_chunks.into_iter().enumerate() {
        if i == chunk_num - 1 {
            // This is last chunk
            // we do not actually have to delete these files, as
            // our very first merge should not be with an empty
            // working copy
            break;
        }

        let changeset_args = delete_commits_changeset_args_factory(StackPosition(i));
        let file_changes: BTreeMap<MPath, _> =
            mpath_chunk.into_iter().map(|mp| (mp, None)).collect();
        info!(
            ctx.logger(),
            "Creating delete commit #{} with {:?} (deleting {} files)",
            i,
            changeset_args,
            file_changes.len()
        );
        let delete_cs_id =
            create_and_save_bonsai(ctx, repo, vec![parent], file_changes, changeset_args).await?;
        info!(ctx.logger(), "Done creating delete commit #{}", i);
        delete_commits.push(delete_cs_id);

        // move one step forward
        parent = delete_cs_id;
    }

    Ok(PreMergeDelete { delete_commits })
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::common::ChangesetArgs;
    use cloned::cloned;
    use fbinit::FacebookInit;
    use fixtures::linear;
    use mononoke_types::DateTime;
    use std::collections::HashSet;
    use tests_utils::{resolve_cs_id, CreateCommitContext};

    #[fbinit::compat_test]
    async fn test_create_pre_merge_delete(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
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

    #[fbinit::compat_test]
    async fn test_create_pre_merge_delete_with_base(fb: FacebookInit) -> Result<(), Error> {
        let repo = linear::getrepo(fb).await;
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
