/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use changesets::ChangesetsRef;
use context::CoreContext;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use mercurial_types::NonRootMPath;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use phases::PhasesRef;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use slog::info;
use sorted_vector_map::SortedVectorMap;

use crate::chunking::Chunker;

pub trait Repo =
    ChangesetsRef + RepoBlobstoreRef + PhasesRef + BookmarksRef + RepoDerivedDataRef + Send + Sync;

#[derive(Clone, Debug)]
pub struct ChangesetArgs {
    pub author: String,
    pub message: String,
    pub datetime: DateTime,
    pub bookmark: Option<BookmarkKey>,
    pub mark_public: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StackPosition(pub usize);

/// For creating stacks of changesets
pub trait ChangesetArgsFactory = Fn(StackPosition) -> ChangesetArgs;

pub async fn create_save_and_generate_hg_changeset(
    ctx: &CoreContext,
    repo: &impl Repo,
    parents: Vec<ChangesetId>,
    file_changes: SortedVectorMap<NonRootMPath, FileChange>,
    changeset_args: ChangesetArgs,
) -> Result<HgChangesetId, Error> {
    let bcs_id = create_and_save_bonsai(ctx, repo, parents, file_changes, changeset_args).await?;
    generate_hg_changeset(ctx, repo, bcs_id).await
}

pub async fn create_and_save_bonsai(
    ctx: &CoreContext,
    repo: &impl Repo,
    parents: Vec<ChangesetId>,
    file_changes: SortedVectorMap<NonRootMPath, FileChange>,
    changeset_args: ChangesetArgs,
) -> Result<ChangesetId, Error> {
    let ChangesetArgs {
        author,
        message,
        datetime,
        bookmark: maybe_bookmark,
        mark_public,
    } = changeset_args;
    let bcs = create_bonsai_changeset_only(parents, file_changes, author, message, datetime)?;
    let bcs_id = save_and_maybe_mark_public(ctx, repo, bcs, mark_public).await?;

    if let Some(bookmark) = maybe_bookmark {
        create_bookmark(ctx, repo, bookmark, bcs_id).await?;
    }

    Ok(bcs_id)
}

async fn save_and_maybe_mark_public(
    ctx: &CoreContext,
    repo: &impl Repo,
    bcs: BonsaiChangeset,
    mark_public: bool,
) -> Result<ChangesetId, Error> {
    let bcs_id = bcs.get_changeset_id();
    changesets_creation::save_changesets(ctx, repo, vec![bcs]).await?;

    if mark_public {
        repo.phases()
            .add_reachable_as_public(ctx, vec![bcs_id])
            .await?;
        info!(ctx.logger(), "Marked as public {:?}", bcs_id);
    }
    Ok(bcs_id)
}

async fn generate_hg_changeset(
    ctx: &CoreContext,
    repo: &impl Repo,
    bcs_id: ChangesetId,
) -> Result<HgChangesetId, Error> {
    info!(ctx.logger(), "Generating an HG equivalent of {:?}", bcs_id);
    let hg_cs_id = repo.derive_hg_changeset(ctx, bcs_id).await?;

    info!(
        ctx.logger(),
        "Hg equivalent of {:?} is: {:?}", bcs_id, hg_cs_id
    );
    Ok(hg_cs_id)
}

async fn create_bookmark(
    ctx: &CoreContext,
    repo: &impl BookmarksRef,
    bookmark: BookmarkKey,
    bcs_id: ChangesetId,
) -> Result<(), Error> {
    info!(
        ctx.logger(),
        "Setting bookmark {:?} to point to {:?}", bookmark, bcs_id
    );
    let mut transaction = repo.bookmarks().create_transaction(ctx.clone());
    transaction.force_set(&bookmark, bcs_id, BookmarkUpdateReason::ManualMove)?;

    let commit_result = transaction.commit().await?;

    if !commit_result {
        Err(format_err!("Logical failure while setting {:?}", bookmark))
    } else {
        info!(ctx.logger(), "Setting bookmark {:?} finished", bookmark);
        Ok(())
    }
}

fn create_bonsai_changeset_only(
    parents: Vec<ChangesetId>,
    file_changes: SortedVectorMap<NonRootMPath, FileChange>,
    author: String,
    message: String,
    datetime: DateTime,
) -> Result<BonsaiChangeset, Error> {
    BonsaiChangesetMut {
        parents,
        author,
        author_date: datetime,
        committer: None,
        committer_date: None,
        message,
        hg_extra: Default::default(),
        git_extra_headers: None,
        git_tree_hash: None,
        file_changes,
        is_snapshot: false,
        git_annotated_tag: None,
    }
    .freeze()
}

pub async fn delete_files_in_chunks<'a>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    parent_bcs_id: ChangesetId,
    mpaths: Vec<NonRootMPath>,
    chunker: &Chunker<NonRootMPath>,
    delete_commits_changeset_args_factory: &impl ChangesetArgsFactory,
    skip_last_chunk: bool,
) -> Result<Vec<ChangesetId>, Error> {
    info!(ctx.logger(), "Chunking mpaths");
    let mpath_chunks: Vec<Vec<NonRootMPath>> = chunker(mpaths);
    info!(ctx.logger(), "Done chunking working copy contents");

    let mut delete_commits: Vec<ChangesetId> = Vec::new();
    let mut parent = parent_bcs_id;
    let chunk_num = mpath_chunks.len();
    for (i, mpath_chunk) in mpath_chunks.into_iter().enumerate() {
        if i == chunk_num - 1 && skip_last_chunk {
            break;
        }

        let changeset_args = delete_commits_changeset_args_factory(StackPosition(i));
        let file_changes: SortedVectorMap<NonRootMPath, _> = mpath_chunk
            .into_iter()
            .map(|mp| (mp, FileChange::Deletion))
            .collect();
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

    Ok(delete_commits)
}
