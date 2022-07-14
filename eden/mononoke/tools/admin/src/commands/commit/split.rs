/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;

use anyhow::bail;
use anyhow::Result;
use blobstore::Loadable;
use changesets_creation::save_changesets;
use clap::ArgGroup;
use clap::Args;
use context::CoreContext;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::MPath;
use repo_blobstore::RepoBlobstoreRef;

use super::Repo;
use crate::commit_id::parse_commit_id;

#[derive(Args)]
#[clap(group(ArgGroup::new("file-size-and-num").args(&["commit-file-size", "commit-file-num"]).multiple(true)))]
pub struct CommitSplitArgs {
    /// Commit ID to split
    commit_id: String,

    /// Target sum of the size of files in each commit (in bytes)
    #[clap(long)]
    commit_file_size: Option<u64>,

    /// Target number of files in each commit
    #[clap(long)]
    commit_file_num: Option<u64>,
}

pub async fn split(ctx: &CoreContext, repo: &Repo, split_args: CommitSplitArgs) -> Result<()> {
    let cs_id = parse_commit_id(ctx, repo, &split_args.commit_id).await?;

    let stack = split_commit(
        ctx,
        repo,
        cs_id,
        split_args.commit_file_size,
        split_args.commit_file_num,
    )
    .await?;

    println!("Split {} into {} commits", cs_id, stack.len());

    Ok(())
}

async fn split_commit(
    ctx: &CoreContext,
    repo: &Repo,
    cs_id: ChangesetId,
    commit_file_size: Option<u64>,
    commit_file_num: Option<u64>,
) -> Result<Vec<ChangesetId>> {
    let bcs = cs_id.load(ctx, repo.repo_blobstore()).await?;

    if bcs.is_merge() {
        bail!("splitting merges is not supported!");
    }

    let mut parent = bcs.parents().next();
    let mut current_file_changes = BTreeMap::new();
    let mut current_file_size: u64 = 0;

    // We want copy/move sources and destination to be in one commit.
    // So let's group them here
    let mut file_groups: BTreeMap<MPath, Vec<(MPath, FileChange)>> = BTreeMap::new();
    let file_changes = bcs.file_changes_map();
    for (path, fc) in file_changes.iter() {
        if let FileChange::Change(ft) = fc {
            if let Some((from_path, _)) = ft.copy_from() {
                file_groups
                    .entry(from_path.clone())
                    .or_default()
                    .push((path.clone(), fc.clone()));
                continue;
            }
        }

        file_groups
            .entry(path.clone())
            .or_default()
            .push((path.clone(), fc.clone()));
    }

    let mut result = vec![];
    for file_group in file_groups.values() {
        let mut should_create_new_commit = false;
        if let Some(commit_file_num) = commit_file_num {
            if commit_file_num <= current_file_changes.len() as u64 {
                should_create_new_commit = true;
            }
        }

        if let Some(commit_file_size) = commit_file_size {
            if commit_file_size <= current_file_size {
                should_create_new_commit = true;
            }
        }

        if should_create_new_commit {
            let num_of_files = current_file_changes.len();
            let cs_id = create_new_commit(
                ctx,
                repo,
                bcs.clone(),
                parent.clone(),
                &mut current_file_changes,
            )
            .await?;
            parent = Some(cs_id);
            println!(
                "{} size: {} files: {}",
                cs_id, current_file_size, num_of_files
            );
            current_file_changes.clear();
            current_file_size = 0;
            result.extend(parent);
        }

        for (path, fc) in file_group {
            let new_fc = modify_file_change_parent(path, fc, parent)?;
            if let FileChange::Change(fc) = &new_fc {
                current_file_size += fc.size();
            }
            current_file_changes.insert(path.clone(), new_fc);
        }
    }

    if !current_file_changes.is_empty() {
        let num_of_files = current_file_changes.len();
        let cs_id = create_new_commit(
            ctx,
            repo,
            bcs.clone(),
            parent.clone(),
            &mut current_file_changes,
        )
        .await?;
        result.push(cs_id);
        println!(
            "{} size: {} files: {}",
            cs_id, current_file_size, num_of_files
        );
    }

    Ok(result)
}

fn modify_file_change_parent(
    path: &MPath,
    fc: &FileChange,
    parent: Option<ChangesetId>,
) -> Result<FileChange> {
    let new_fc = match fc {
        FileChange::Change(fc) => {
            // We need to fix copy info and change the parent
            if let Some((from_path, _)) = fc.copy_from() {
                let copy_from = if let Some(parent) = parent {
                    (from_path.clone(), parent)
                } else {
                    bail!(
                        "invalid bonsai changeset - it's a root commit, but has copy info for {}",
                        path
                    );
                };

                FileChange::Change(fc.with_new_copy_from(Some(copy_from)))
            } else {
                FileChange::Change(fc.clone())
            }
        }
        FileChange::Deletion => fc.clone(),
        FileChange::UntrackedDeletion | FileChange::UntrackedChange(_) => {
            bail!("cannot split snapshots");
        }
    };
    Ok(new_fc)
}

async fn create_new_commit(
    ctx: &CoreContext,
    repo: &Repo,
    bcs: BonsaiChangeset,
    parent: Option<ChangesetId>,
    current_file_changes: &mut BTreeMap<MPath, FileChange>,
) -> Result<ChangesetId> {
    let mut new_bcs = bcs.clone().into_mut();
    new_bcs.parents = parent.into_iter().collect();
    new_bcs.file_changes = std::mem::take(current_file_changes).into();
    let new_bcs = new_bcs.freeze()?;
    let cs_id = new_bcs.get_changeset_id();
    save_changesets(ctx, repo, vec![new_bcs]).await?;
    Ok(cs_id)
}

#[cfg(test)]
mod test {
    use super::*;
    use bonsai_git_mapping::BonsaiGitMapping;
    use bonsai_globalrev_mapping::BonsaiGlobalrevMapping;
    use bonsai_hg_mapping::BonsaiHgMapping;
    use bonsai_svnrev_mapping::BonsaiSvnrevMapping;
    use bookmarks::Bookmarks;
    use changeset_fetcher::ChangesetFetcher;
    use changesets::Changesets;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use maplit::hashmap;
    use repo_blobstore::RepoBlobstore;
    use repo_blobstore::RepoBlobstoreRef;
    use repo_derived_data::RepoDerivedData;
    use tests_utils::list_working_copy_utf8;
    use tests_utils::CreateCommitContext;

    #[facet::container]
    struct BasicTestRepo {
        #[delegate(
            dyn BonsaiHgMapping,
            dyn BonsaiGitMapping,
            dyn BonsaiGlobalrevMapping,
            dyn BonsaiSvnrevMapping,
            dyn Changesets,
            RepoBlobstore,
        )]
        repo: Repo,

        #[facet]
        bookmarks: dyn Bookmarks,

        #[facet]
        filestore_config: FilestoreConfig,

        #[facet]
        repo_derived_data: RepoDerivedData,

        #[facet]
        changeset_fetcher: dyn ChangesetFetcher,
    }

    #[fbinit::test]
    async fn test_split_commit_simple(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;

        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("first", "a")
            .add_file("second", "b")
            .add_file("third", "c")
            .commit()
            .await?;

        let split = split_commit(
            &ctx,
            &repo.repo,
            root,
            None,    // file size
            Some(1), // file num
        )
        .await?;

        assert_eq!(split.len(), 3);
        {
            // Make sure it has the right contents
            let cs_id = *split.last().unwrap();
            let wc = list_working_copy_utf8(&ctx, &repo, cs_id).await?;
            assert_eq!(
                wc,
                hashmap! {
                    MPath::new("first")? => "a".to_string(),
                    MPath::new("second")? => "b".to_string(),
                    MPath::new("third")? => "c".to_string(),
                }
            );

            let bcs = split[0].load(&ctx, repo.repo_blobstore()).await?;
            let parents: Vec<ChangesetId> = vec![];
            assert_eq!(
                parents,
                bcs.parents().into_iter().collect::<Vec<ChangesetId>>()
            );
            let bcs = split[1].load(&ctx, repo.repo_blobstore()).await?;
            assert_eq!(
                vec![split[0]],
                bcs.parents().into_iter().collect::<Vec<_>>()
            );
            let bcs = split[2].load(&ctx, repo.repo_blobstore()).await?;
            assert_eq!(
                vec![split[1]],
                bcs.parents().into_iter().collect::<Vec<_>>()
            );
        }

        // Now split by file size
        let split = split_commit(
            &ctx,
            &repo.repo,
            root,
            Some(1), // file size
            None,    // file num
        )
        .await?;

        assert_eq!(split.len(), 3);

        let split = split_commit(
            &ctx,
            &repo.repo,
            root,
            None,    // file size
            Some(3), // file num
        )
        .await?;

        assert_eq!(split.len(), 1);
        {
            let cs_id = *split.last().unwrap();
            // Make sure it has the right contents
            let wc = list_working_copy_utf8(&ctx, &repo, cs_id).await?;
            assert_eq!(
                wc,
                hashmap! {
                    MPath::new("first")? => "a".to_string(),
                    MPath::new("second")? => "b".to_string(),
                    MPath::new("third")? => "c".to_string(),
                }
            );
        }

        Ok(())
    }

    #[fbinit::test]
    async fn test_split_commit_with_renames(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;

        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("first", "a")
            .commit()
            .await?;
        let to_split = CreateCommitContext::new(&ctx, &repo, vec![root])
            .add_file_with_copy_info("second", "a", (root, "first"))
            .delete_file("first")
            .add_file("third", "c")
            .commit()
            .await?;

        let split = split_commit(
            &ctx,
            &repo.repo,
            to_split,
            None,    // file size
            Some(1), // file num
        )
        .await?;

        assert_eq!(split.len(), 2);
        {
            // Make sure it has the right contents
            let cs_id = *split.last().unwrap();
            let wc = list_working_copy_utf8(&ctx, &repo, cs_id).await?;
            assert_eq!(
                wc,
                hashmap! {
                    MPath::new("second")? => "a".to_string(),
                    MPath::new("third")? => "c".to_string(),
                }
            );
        }

        Ok(())
    }

    #[fbinit::test]
    async fn test_split_commit_renamed_to_multiple_dest(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BasicTestRepo = test_repo_factory::build_empty(fb)?;

        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("first", "a")
            .commit()
            .await?;
        let to_split = CreateCommitContext::new(&ctx, &repo, vec![root])
            .delete_file("first")
            .add_file_with_copy_info("second", "a", (root, "first"))
            .add_file_with_copy_info("third", "c", (root, "first"))
            .commit()
            .await?;

        let split = split_commit(
            &ctx,
            &repo.repo,
            to_split,
            None,    // file size
            Some(1), // file num
        )
        .await?;

        assert_eq!(split.len(), 1);
        {
            // Make sure it has the right contents
            let cs_id = *split.last().unwrap();
            let wc = list_working_copy_utf8(&ctx, &repo, cs_id).await?;
            assert_eq!(
                wc,
                hashmap! {
                    MPath::new("second")? => "a".to_string(),
                    MPath::new("third")? => "c".to_string(),
                }
            );
            let bcs = cs_id.load(&ctx, repo.repo_blobstore()).await?;
            assert_eq!(bcs.file_changes_map().len(), 3);
        }

        Ok(())
    }
}
