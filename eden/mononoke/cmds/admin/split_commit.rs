/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::error::SubcommandError;

use anyhow::{anyhow, Error};
use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobstore::Loadable;
use clap::{App, Arg, ArgMatches, SubCommand};
use cmdlib::{
    args::{self, MononokeMatches},
    helpers::csid_resolve,
};
use context::CoreContext;
use fbinit::FacebookInit;
use mononoke_types::{BonsaiChangeset, ChangesetId, FileChange, MPath};
use slog::{info, Logger};
use std::collections::BTreeMap;

pub const SPLIT_COMMIT: &str = "split-commit";
const ARG_HASH_OR_BOOKMARK: &str = "hash-or-bookmark";
const ARG_COMMIT_FILE_SIZE: &str = "commit-file-size";
const ARG_COMMIT_FILE_NUM: &str = "commit-file-num";

pub fn build_subcommand<'a, 'b>() -> App<'a, 'b> {
    SubCommand::with_name(SPLIT_COMMIT)
        .about("command to split bonsai commit in a stack of commits")
        .long_about("This command splits a bonsai into a stack of commits while trying to maintain \
        the limits on number of files and size of all files in a commit. However these limits are not strict \
        i.e. resulting commits might have larger size and/or number of files. For example, if input commit has
        a file which size is larger than file size limit, then obviously one of the commits will be larger than
        the limit. Also we group copy/move sources and destinations in a single commit, which might also make
        one of the commits to go above the limit.")
        .arg(
            Arg::with_name(ARG_COMMIT_FILE_NUM)
                .long(ARG_COMMIT_FILE_NUM)
                .help("target number of files in the commit. Not a strict limit - resulting commit might be bigger")
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_COMMIT_FILE_SIZE)
                .long(ARG_COMMIT_FILE_SIZE)
                .help("target sum of all files sizes in the commit, in bytes. Not a strict limit - resulting commit might be bigger")
                .takes_value(true)
                .required(false),
        )
        .arg(
            Arg::with_name(ARG_HASH_OR_BOOKMARK)
                .help("(hg|bonsai) commit hash or bookmark")
                .takes_value(true)
                .multiple(false)
                .required(true),
        )
}

pub async fn subcommand_split_commit<'a>(
    fb: FacebookInit,
    logger: Logger,
    matches: &'a MononokeMatches<'_>,
    sub_matches: &'a ArgMatches<'_>,
) -> Result<(), SubcommandError> {
    let ctx = CoreContext::new_with_logger(fb, logger.clone());
    let repo: BlobRepo = args::open_repo(fb, &logger, &matches).await?;

    let commit_file_size = args::get_u64_opt(sub_matches, ARG_COMMIT_FILE_SIZE);
    let commit_file_num = args::get_u64_opt(sub_matches, ARG_COMMIT_FILE_NUM);

    let hash_or_bm = sub_matches.value_of(ARG_HASH_OR_BOOKMARK).ok_or_else(|| {
        let err: SubcommandError = anyhow!("--{} not set", ARG_HASH_OR_BOOKMARK).into();
        err
    })?;
    let cs_id = csid_resolve(&ctx, repo.clone(), hash_or_bm).await?;

    let result = split_commit(&ctx, &repo, cs_id, commit_file_size, commit_file_num).await?;

    info!(
        ctx.logger(),
        "commits are printed from ancestors to descendants"
    );
    for cs_id in result {
        println!("{}", cs_id);
    }

    Ok(())
}

async fn split_commit(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
    commit_file_size: Option<u64>,
    commit_file_num: Option<u64>,
) -> Result<Vec<ChangesetId>, Error> {
    let bcs = cs_id
        .load(&ctx, &repo.get_blobstore())
        .await
        .map_err(Error::from)?;

    if bcs.is_merge() {
        return Err(anyhow!("splitting merges is not supported!").into());
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
                &ctx,
                &repo,
                bcs.clone(),
                parent.clone(),
                &mut current_file_changes,
            )
            .await?;
            parent = Some(cs_id);
            info!(
                ctx.logger(),
                "{}, size: {}, number of files: {}", cs_id, current_file_size, num_of_files,
            );
            current_file_changes.clear();
            current_file_size = 0;
            result.extend(parent);
        }

        for (path, fc) in file_group {
            let new_fc = fixup_file_change(&path, &fc, &parent)?;
            if let FileChange::Change(fc) = &new_fc {
                current_file_size += fc.size();
            }
            current_file_changes.insert(path.clone(), new_fc);
        }
    }

    if !current_file_changes.is_empty() {
        let num_of_files = current_file_changes.len();
        let cs_id = create_new_commit(
            &ctx,
            &repo,
            bcs.clone(),
            parent.clone(),
            &mut current_file_changes,
        )
        .await?;
        result.push(cs_id);
        info!(
            ctx.logger(),
            "{}, size: {}, number of files: {}", cs_id, current_file_size, num_of_files
        );
    }

    Ok(result)
}

fn fixup_file_change(
    path: &MPath,
    fc: &FileChange,
    parent: &Option<ChangesetId>,
) -> Result<FileChange, Error> {
    let new_fc = match fc {
        FileChange::Change(fc) => {
            // current_file_size += fc.size();
            // We need to fix copy info and change the parent
            if let Some((from_path, _)) = fc.copy_from() {
                let copy_from = if let Some(parent) = parent {
                    (from_path.clone(), *parent)
                } else {
                    return Err(anyhow!(
                        "invalid bonsai changeset - it's a root commit, but has copy info for {}",
                        path
                    ));
                };

                FileChange::Change(fc.with_new_copy_from(Some(copy_from)))
            } else {
                FileChange::Change(fc.clone())
            }
        }
        FileChange::Deletion => fc.clone(),
        FileChange::UntrackedDeletion | FileChange::UntrackedChange(_) => {
            return Err(anyhow!("cannot split snapshots!").into());
        }
    };
    Ok(new_fc)
}

async fn create_new_commit(
    ctx: &CoreContext,
    repo: &BlobRepo,
    bcs: BonsaiChangeset,
    parent: Option<ChangesetId>,
    current_file_changes: &mut BTreeMap<MPath, FileChange>,
) -> Result<ChangesetId, Error> {
    let mut new_bcs = bcs.clone().into_mut();
    let mut parents = vec![];
    parents.extend(parent);
    new_bcs.parents = parents;
    new_bcs.file_changes = current_file_changes.drain_filter(|_, _| true).collect();
    let new_bcs = new_bcs.freeze()?;
    let res = new_bcs.get_changeset_id();
    save_bonsai_changesets(vec![new_bcs], ctx.clone(), repo).await?;
    Ok(res)
}

#[cfg(test)]
mod test {
    use super::*;
    use blobrepo_hg::BlobRepoHg;
    use maplit::hashmap;
    use tests_utils::{list_working_copy_utf8, CreateCommitContext};

    #[fbinit::test]
    async fn test_split_commit_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty()?;

        let root = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("first", "a")
            .add_file("second", "b")
            .add_file("third", "c")
            .commit()
            .await?;

        let split = split_commit(
            &ctx,
            &repo,
            root,
            None,    // file size
            Some(1), // file num
        )
        .await?;

        assert_eq!(split.len(), 3);
        {
            // Make sure it's derived correctly
            let cs_id = *split.last().unwrap();
            repo.get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
                .await?;
            let wc = list_working_copy_utf8(&ctx, &repo, cs_id).await?;
            assert_eq!(
                wc,
                hashmap! {
                    MPath::new("first")? => "a".to_string(),
                    MPath::new("second")? => "b".to_string(),
                    MPath::new("third")? => "c".to_string(),
                }
            );

            let bcs = split[0].load(&ctx, &repo.get_blobstore()).await?;
            let parents: Vec<ChangesetId> = vec![];
            assert_eq!(
                parents,
                bcs.parents().into_iter().collect::<Vec<ChangesetId>>()
            );
            let bcs = split[1].load(&ctx, &repo.get_blobstore()).await?;
            assert_eq!(
                vec![split[0]],
                bcs.parents().into_iter().collect::<Vec<_>>()
            );
            let bcs = split[2].load(&ctx, &repo.get_blobstore()).await?;
            assert_eq!(
                vec![split[1]],
                bcs.parents().into_iter().collect::<Vec<_>>()
            );
        }

        // Now split by file size
        let split = split_commit(
            &ctx,
            &repo,
            root,
            Some(1), // file size
            None,    // file num
        )
        .await?;

        assert_eq!(split.len(), 3);

        let split = split_commit(
            &ctx,
            &repo,
            root,
            None,    // file size
            Some(3), // file num
        )
        .await?;

        assert_eq!(split.len(), 1);
        {
            let cs_id = *split.last().unwrap();
            // Make sure it's derived correctly
            repo.get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
                .await?;
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
    async fn test_split_commit_with_renames(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty()?;

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
            &repo,
            to_split,
            None,    // file size
            Some(1), // file num
        )
        .await?;

        assert_eq!(split.len(), 2);
        {
            // Make sure it's derived correctly
            let cs_id = *split.last().unwrap();
            repo.get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
                .await?;
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
    async fn test_split_commit_renamed_to_multiple_dest(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty()?;

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
            &repo,
            to_split,
            None,    // file size
            Some(1), // file num
        )
        .await?;

        assert_eq!(split.len(), 1);
        {
            // Make sure it's derived correctly
            let cs_id = *split.last().unwrap();
            repo.get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
                .await?;
            let wc = list_working_copy_utf8(&ctx, &repo, cs_id).await?;
            assert_eq!(
                wc,
                hashmap! {
                    MPath::new("second")? => "a".to_string(),
                    MPath::new("third")? => "c".to_string(),
                }
            );
            let bcs = cs_id.load(&ctx, &repo.get_blobstore()).await?;
            assert_eq!(bcs.file_changes_map().len(), 3);
        }


        Ok(())
    }
}
