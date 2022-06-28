/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use blobrepo::save_bonsai_changesets;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use bookmarks::BookmarkUpdateReason;
use commit_transformation::copy_file_contents;
use commit_transformation::rewrite_stack_no_merges;
use context::CoreContext;
use cross_repo_sync::mover_to_multi_mover;
use cross_repo_sync::types::Source;
use cross_repo_sync::types::Target;
use futures::compat::Stream01CompatExt;
use futures::stream;
use futures::StreamExt;
use futures::TryStreamExt;
use mononoke_api_types::InnerRepo;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use reachabilityindex::LeastCommonAncestorsHint;
use revset::RangeNodeStream;
use slog::info;
use std::collections::HashMap;

use crate::common::decode_latest_synced_state_extras;
use crate::common::encode_latest_synced_state_extras;
use crate::common::get_mover_and_reverse_mover;

const CHUNK_SIZE: usize = 100;

pub async fn tail_once(
    ctx: &CoreContext,
    source_repos: Vec<InnerRepo>,
    hyper_repo: BlobRepo,
    source_bookmark: &Source<BookmarkName>,
    hyper_repo_bookmark: &Target<BookmarkName>,
) -> Result<(), Error> {
    let source_repos_and_latest_synced_commits =
        find_latest_synced_commits(ctx, &source_repos, &hyper_repo, hyper_repo_bookmark).await?;

    let mut latest_replayed_state = HashMap::new();
    for (source_repo, latest_synced) in &source_repos_and_latest_synced_commits {
        latest_replayed_state.insert(source_repo.blob_repo.name().to_string(), *latest_synced);
    }

    for (source_repo, latest_synced_commit) in source_repos_and_latest_synced_commits {
        let commits_to_replay =
            find_commits_to_replay(ctx, &source_repo, latest_synced_commit, source_bookmark)
                .await?;

        for chunk in commits_to_replay.chunks(CHUNK_SIZE) {
            sync_commits(
                ctx,
                &source_repo,
                &hyper_repo,
                chunk,
                hyper_repo_bookmark,
                &mut latest_replayed_state,
            )
            .await?;
        }
    }

    Ok(())
}

async fn find_latest_synced_commits(
    ctx: &CoreContext,
    source_repos: &[InnerRepo],
    hyper_repo: &BlobRepo,
    bookmark_name: &Target<BookmarkName>,
) -> Result<Vec<(InnerRepo, ChangesetId)>, Error> {
    let hyper_repo_tip_cs_id = hyper_repo
        .get_bonsai_bookmark(ctx.clone(), bookmark_name)
        .await?
        .ok_or_else(|| anyhow!("{} bookmark not found in hyper repo", bookmark_name))?;

    let hyper_repo_tip = hyper_repo_tip_cs_id
        .load(ctx, &hyper_repo.get_blobstore())
        .await?;

    let latest_synced_commits = decode_latest_synced_state_extras(hyper_repo_tip.extra())?;

    let mut res = vec![];
    for source_repo in source_repos {
        let latest = latest_synced_commits
            .get(source_repo.blob_repo.name().as_str())
            .ok_or_else(|| {
                anyhow!(
                    "not found latest cs id for {}",
                    source_repo.blob_repo.name()
                )
            })?;

        res.push((source_repo.clone(), *latest));
    }

    Ok(res)
}

async fn find_commits_to_replay(
    ctx: &CoreContext,
    source_repo: &InnerRepo,
    latest_synced_cs_id: ChangesetId,
    bookmark_name: &Source<BookmarkName>,
) -> Result<Vec<ChangesetId>, Error> {
    let source_repo_tip_cs_id = source_repo
        .blob_repo
        .get_bonsai_bookmark(ctx.clone(), bookmark_name)
        .await?
        .ok_or_else(|| anyhow!("{} bookmark not found in source repo", bookmark_name))?;

    if latest_synced_cs_id == source_repo_tip_cs_id {
        return Ok(vec![]);
    }

    let is_ancestor = source_repo
        .skiplist_index
        .is_ancestor(
            ctx,
            &source_repo.blob_repo.get_changeset_fetcher(),
            latest_synced_cs_id,
            source_repo_tip_cs_id,
        )
        .await?;

    // TODO(stash): add instructions on what to do here
    if !is_ancestor {
        return Err(anyhow!(
            "non-forward bookmark move of {} in {}",
            bookmark_name,
            source_repo.blob_repo.name()
        ));
    }

    let cs_ids = RangeNodeStream::new(
        ctx.clone(),
        source_repo.blob_repo.get_changeset_fetcher(),
        latest_synced_cs_id,
        source_repo_tip_cs_id,
    )
    .compat()
    .try_collect::<Vec<_>>()
    .await?;

    // csids are from descendants to ancestors hence we need to reverse.
    // also they include latest_synced_cs_id, so we need to skip it
    let cs_ids = cs_ids.into_iter().rev().skip(1).collect::<Vec<_>>();
    info!(
        ctx.logger(),
        "found {} commits to sync from {} repo",
        cs_ids.len(),
        source_repo.blob_repo.name()
    );

    Ok(cs_ids)
}

async fn sync_commits(
    ctx: &CoreContext,
    source_repo: &InnerRepo,
    hyper_repo: &BlobRepo,
    cs_ids: &[ChangesetId],
    bookmark_name: &Target<BookmarkName>,
    latest_synced_state: &mut HashMap<String, ChangesetId>,
) -> Result<(), Error> {
    if cs_ids.is_empty() {
        return Ok(());
    }

    let hyper_repo_tip_cs_id = hyper_repo
        .get_bonsai_bookmark(ctx.clone(), bookmark_name)
        .await?
        .ok_or_else(|| anyhow!("{} bookmark not found in hyper repo", bookmark_name))?;

    let blobstore = source_repo.blob_repo.get_blobstore();
    let bcss = stream::iter(cs_ids)
        .map(|cs_id| cs_id.load(ctx, &blobstore))
        .buffered(CHUNK_SIZE)
        .try_collect::<Vec<_>>()
        .await?;

    for cs in &bcss {
        if cs.is_merge() {
            return Err(anyhow!("syncing merges is not implemented yet"));
        }
    }

    info!(
        ctx.logger(),
        "preparing {} commits from {:?} to {:?}, repo {}",
        bcss.len(),
        bcss.get(0).map(|cs| cs.get_changeset_id()),
        bcss.last().map(|cs| cs.get_changeset_id()),
        source_repo.blob_repo.name()
    );

    let mut files_to_sync = vec![];
    for bcs in &bcss {
        let new_files_to_sync = bcs.file_changes().filter_map(|(_, change)| match change {
            FileChange::Change(tc) => Some(tc.content_id()),
            FileChange::UntrackedChange(uc) => Some(uc.content_id()),
            FileChange::Deletion | FileChange::UntrackedDeletion => None,
        });
        files_to_sync.extend(new_files_to_sync);
    }
    info!(
        ctx.logger(),
        "started syncing {} file contents",
        files_to_sync.len()
    );
    copy_file_contents(
        ctx,
        &source_repo.blob_repo,
        hyper_repo,
        files_to_sync,
        |i| {
            info!(ctx.logger(), "copied {} files", i);
        },
    )
    .await?;
    info!(ctx.logger(), "synced file contents");

    let (mover, _) = get_mover_and_reverse_mover(&source_repo.blob_repo)?;
    let mover = mover_to_multi_mover(mover);
    let rewritten_commits = rewrite_stack_no_merges(
        ctx,
        bcss,
        hyper_repo_tip_cs_id,
        mover.clone(),
        source_repo.blob_repo.clone(),
        None, // force_first_parent
        |(cs_id, mut rewritten_commit)| {
            latest_synced_state.insert(source_repo.blob_repo.name().to_string(), cs_id);
            let extra = encode_latest_synced_state_extras(latest_synced_state);
            rewritten_commit.extra = extra;
            // overwrite the date so that it's closer to when the commit actually
            // created in hyper repo rather than when it's created in source repo.
            // This makes it easier to track e.g. derivation delay.
            rewritten_commit.author_date = DateTime::now();
            rewritten_commit
        },
    )
    .await?;

    let rewritten_commits = rewritten_commits
        .into_iter()
        .map(|maybe_bcs| maybe_bcs.context("unexpected empty commit after rewrite"))
        .collect::<Result<Vec<_>, Error>>()?;

    let latest_commit = rewritten_commits
        .last()
        .map(|cs| cs.get_changeset_id())
        .ok_or_else(|| anyhow!("no commits found!"))?;
    save_bonsai_changesets(rewritten_commits, ctx.clone(), hyper_repo).await?;

    let mut txn = hyper_repo.update_bookmark_transaction(ctx.clone());
    txn.update(
        bookmark_name,
        latest_commit,
        hyper_repo_tip_cs_id,
        BookmarkUpdateReason::ManualMove,
    )?;
    let success = txn.commit().await?;
    if !success {
        return Err(anyhow!(
            "failed to move {} bookmark in hyper repo",
            bookmark_name
        ));
    }

    Ok(())
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::add_source_repo::add_source_repo;
    use fbinit::FacebookInit;
    use maplit::hashmap;
    use mononoke_types::MPath;
    use mononoke_types::RepositoryId;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::bookmark;
    use tests_utils::list_working_copy_utf8;
    use tests_utils::resolve_cs_id;
    use tests_utils::CreateCommitContext;

    #[fbinit::test]
    async fn test_sync_commit(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test_repo_factory = TestRepoFactory::new(fb)?;

        let source_repo: InnerRepo = test_repo_factory
            .with_id(RepositoryId::new(0))
            .with_name("source_repo")
            .build()?;
        let hyper_repo: BlobRepo = test_repo_factory
            .with_id(RepositoryId::new(2))
            .with_name("hyper_repo")
            .build()?;

        // Create a commit in hyper repo
        let root_cs_id = CreateCommitContext::new_root(&ctx, &hyper_repo)
            .add_file("README.md", "readme")
            .commit()
            .await?;
        bookmark(&ctx, &hyper_repo, "main")
            .set_to(root_cs_id)
            .await?;

        // Create a commit in source repo that will be synced;
        let root_cs_id = CreateCommitContext::new_root(&ctx, &source_repo.blob_repo)
            .add_file("1.txt", "start")
            .commit()
            .await?;
        let second_cs_id = CreateCommitContext::new(&ctx, &source_repo.blob_repo, vec![root_cs_id])
            .add_file("1.txt", "content")
            .commit()
            .await?;

        bookmark(&ctx, &source_repo.blob_repo, "main")
            .set_to(second_cs_id)
            .await?;

        let mut latest_synced_state = HashMap::new();
        sync_commits(
            &ctx,
            &source_repo,
            &hyper_repo,
            &[second_cs_id],
            &Target(BookmarkName::new("main")?),
            &mut latest_synced_state,
        )
        .await?;

        assert_eq!(
            list_working_copy_utf8(
                &ctx,
                &hyper_repo,
                resolve_cs_id(&ctx, &hyper_repo, "main").await?,
            )
            .await?,
            hashmap! {
                MPath::new("README.md")? => "readme".to_string(),
                MPath::new("source_repo/1.txt")? => "content".to_string(),
            }
        );

        Ok(())
    }

    #[fbinit::test]
    async fn test_tail_once_simple(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut test_repo_factory = TestRepoFactory::new(fb)?;

        let source_repo: InnerRepo = test_repo_factory
            .with_id(RepositoryId::new(0))
            .with_name("source_repo")
            .build()?;
        let hyper_repo: BlobRepo = test_repo_factory
            .with_id(RepositoryId::new(2))
            .with_name("hyper_repo")
            .build()?;

        let book = Source(BookmarkName::new("main")?);
        let hyper_repo_book = Target(BookmarkName::new("hyper_repo_main")?);
        // Create a commit in hyper repo
        let root_cs_id = CreateCommitContext::new_root(&ctx, &hyper_repo)
            .add_file("README.md", "readme")
            .commit()
            .await?;
        bookmark(&ctx, &hyper_repo, hyper_repo_book.0.clone())
            .set_to(root_cs_id)
            .await?;

        // Add new source repo
        let root_cs_id = CreateCommitContext::new_root(&ctx, &source_repo.blob_repo)
            .add_file("1.txt", "start")
            .commit()
            .await?;
        bookmark(&ctx, &source_repo.blob_repo, book.0.clone())
            .set_to(root_cs_id)
            .await?;

        add_source_repo(
            &ctx,
            &source_repo.blob_repo,
            &hyper_repo,
            &book,
            &hyper_repo_book,
            None,
        )
        .await?;

        // Now sync a single commit
        let second_cs_id = CreateCommitContext::new(&ctx, &source_repo.blob_repo, vec![root_cs_id])
            .add_file("1.txt", "content")
            .commit()
            .await?;
        bookmark(&ctx, &source_repo.blob_repo, book.0.clone())
            .set_to(second_cs_id)
            .await?;

        tail_once(
            &ctx,
            vec![source_repo.clone()],
            hyper_repo.clone(),
            &book,
            &hyper_repo_book,
        )
        .await?;

        assert_eq!(
            list_working_copy_utf8(
                &ctx,
                &hyper_repo,
                resolve_cs_id(&ctx, &hyper_repo, &hyper_repo_book.0).await?,
            )
            .await?,
            hashmap! {
                MPath::new("README.md")? => "readme".to_string(),
                MPath::new("source_repo/1.txt")? => "content".to_string(),
            }
        );

        // Sync two commits, one of them deletes a file that another created
        let create_file_cs_id =
            CreateCommitContext::new(&ctx, &source_repo.blob_repo, vec![second_cs_id])
                .add_file("1.txt", "changed")
                .add_file("newfile", "newfile")
                .commit()
                .await?;
        let delete_file_cs_id =
            CreateCommitContext::new(&ctx, &source_repo.blob_repo, vec![create_file_cs_id])
                .delete_file("newfile")
                .commit()
                .await?;
        bookmark(&ctx, &source_repo.blob_repo, book.0.clone())
            .set_to(delete_file_cs_id)
            .await?;

        tail_once(
            &ctx,
            vec![source_repo.clone()],
            hyper_repo.clone(),
            &book,
            &hyper_repo_book,
        )
        .await?;

        assert_eq!(
            list_working_copy_utf8(
                &ctx,
                &hyper_repo,
                resolve_cs_id(&ctx, &hyper_repo, &hyper_repo_book.0).await?,
            )
            .await?,
            hashmap! {
                MPath::new("README.md")? => "readme".to_string(),
                MPath::new("source_repo/1.txt")? => "changed".to_string(),
            }
        );

        Ok(())
    }
}
