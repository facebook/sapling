/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Imports a Perforce depot into a repo.

use std::collections::HashMap;
use std::pin::pin;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use bonsai_p4_mapping::BonsaiP4Mapping;
use bonsai_p4_mapping::BonsaiP4MappingArc;
use bonsai_p4_mapping::BonsaiP4MappingEntry;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkTransactionError;
use bookmarks::BookmarkTransactionHook;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use bytes::Bytes;
use commit_graph::CommitGraphRef;
use commit_graph::CommitGraphWriterRef;
use context::CoreContext;
use filestore::FilestoreConfigRef;
use filestore::StoreRequest;
use futures::FutureExt;
use futures::Stream;
use futures::TryStreamExt;
use futures::stream;
use import_common::BackfillDerivation;
use import_common::save_and_derive_batch;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::ContentMetadataV2;
use mononoke_types::DerivableType;
use mononoke_types::FileType;
use mononoke_types::NonRootMPath;
use mononoke_types::P4ChangelistId;
use p4_transform::CopySource;
use p4_types::ChangeSource;
use p4_types::Changelist;
use repo_blobstore::RepoBlobstoreRef;
use repo_derived_data::RepoDerivedDataRef;
use repo_identity::RepoIdentityRef;

/// Stores a file revision's content in the repo's filestore. `size` is the size
/// Perforce reports for the revision: content of any other length is rejected.
pub async fn upload_content(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreRef + FilestoreConfigRef),
    size: u64,
    content: impl Stream<Item = Result<Bytes>> + Send,
) -> Result<ContentMetadataV2> {
    filestore::store(
        repo.repo_blobstore(),
        *repo.filestore_config(),
        ctx,
        &StoreRequest::new(size),
        content,
    )
    .await
}

// Only bounds memory: Perforce's default filesys.maxsymlink is 1K, OS limits are 4-8K.
const MAX_SYMLINK_SIZE: u64 = 64 * 1024;

/// Like `upload_content`, but stores a symlink as its bare target.
pub async fn upload_file_content(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreRef + FilestoreConfigRef),
    file_type: FileType,
    size: u64,
    content: impl Stream<Item = Result<Bytes>> + Send,
) -> Result<ContentMetadataV2> {
    if file_type != FileType::Symlink {
        return upload_content(ctx, repo, size, content).await;
    }
    ensure!(
        size <= MAX_SYMLINK_SIZE,
        "symlink of {size} bytes is larger than we support (64 KiB)"
    );
    let printed = read_symlink(size, content).await?;
    let target = symlink_target(size, printed)?;
    upload_content(
        ctx,
        repo,
        target.len() as u64,
        stream::once(async { Ok(target) }),
    )
    .await
}

async fn read_symlink(size: u64, content: impl Stream<Item = Result<Bytes>>) -> Result<Vec<u8>> {
    let mut content = pin!(content);
    let mut printed = Vec::new();
    while let Some(chunk) = content.try_next().await? {
        ensure!(
            (printed.len() + chunk.len()) as u64 <= size,
            "symlink content is longer than Perforce reports ({size} bytes)"
        );
        printed.extend_from_slice(&chunk);
    }
    Ok(printed)
}

// p4 print ends a symlink's target with a newline, and Perforce's size counts it.
fn symlink_target(size: u64, mut printed: Vec<u8>) -> Result<Bytes> {
    ensure!(
        printed.len() as u64 == size,
        "symlink content is {} bytes but Perforce reports {size}",
        printed.len()
    );
    printed
        .pop_if(|last| *last == b'\n')
        .context("symlink content does not end with a newline")?;
    ensure!(
        !printed.ends_with(b"\n"),
        "symlink content ends with more than one newline"
    );
    Ok(Bytes::from(printed))
}

/// Stores the content of every revision in `changelist` that leaves content in the depot
/// at `depot_root`, keyed by depot path as `p4_transform::changeset` takes it.
pub async fn upload_changelist_contents(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreRef + FilestoreConfigRef),
    source: &impl ChangeSource,
    depot_root: &str,
    changelist: &Changelist,
) -> Result<HashMap<String, ContentMetadataV2>> {
    let mut contents = HashMap::new();
    // One file at a time for now.
    for revision in &changelist.files {
        if !revision.action.has_content()
            || p4_transform::repo_path(depot_root, &revision.depot_path)?.is_none()
        {
            continue;
        }
        let name = || format!("{}#{}", revision.depot_path, revision.revision);
        let size = revision
            .size
            .with_context(|| format!("describe gave no size for {}", name()))?;
        let content = source.print(revision).await?;
        let file_type = p4_transform::file_type(&revision.file_type);
        let metadata = upload_file_content(ctx, repo, file_type, size, content)
            .await
            .with_context(|| format!("storing {}", name()))?;
        contents.insert(revision.depot_path.clone(), metadata);
    }
    Ok(contents)
}

/// The source paths of the revisions in `changelist` that were copied into the depot at
/// `depot_root`, keyed by depot path as `p4_transform::changeset` takes them. One
/// `integrations` call covers all of them.
async fn copy_sources(
    source: &impl ChangeSource,
    depot_root: &str,
    changelist: &Changelist,
) -> Result<HashMap<String, NonRootMPath>> {
    let mut revisions = Vec::new();
    for revision in &changelist.files {
        if !p4_transform::can_have_copy_source(revision.action)
            || p4_transform::repo_path(depot_root, &revision.depot_path)?.is_none()
        {
            continue;
        }
        revisions.push(revision);
    }
    let integrations = if revisions.is_empty() {
        HashMap::new()
    } else {
        source.integrations(&revisions).await?
    };
    let mut copy_sources = HashMap::new();
    for revision in revisions {
        let integrations = integrations
            .get(&revision.depot_path)
            .with_context(|| format!("no integration records for {}", revision.depot_path))?;
        if let CopySource::From(path) =
            p4_transform::copy_source(depot_root, revision, integrations)?
        {
            copy_sources.insert(revision.depot_path.clone(), path);
        }
    }
    Ok(copy_sources)
}

async fn build_changeset(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreRef + FilestoreConfigRef),
    source: &impl ChangeSource,
    depot_root: &str,
    parent: Option<ChangesetId>,
    change: u64,
) -> Result<BonsaiChangeset> {
    let changelist = source.describe(change).await?;
    let contents = upload_changelist_contents(ctx, repo, source, depot_root, &changelist).await?;
    let copy_sources = copy_sources(source, depot_root, &changelist).await?;
    Ok(p4_transform::changeset(
        depot_root,
        &changelist,
        parent,
        &contents,
        &copy_sources,
    )?)
}

/// Each of `changes`, in order, with its changeset, each on top of the one before and the
/// first on top of `parent`. Their contents are stored; the changesets themselves are not.
pub async fn build_changesets(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreRef + FilestoreConfigRef),
    source: &impl ChangeSource,
    depot_root: &str,
    mut parent: Option<ChangesetId>,
    changes: &[u64],
) -> Result<Vec<(u64, BonsaiChangeset)>> {
    let mut changesets = Vec::with_capacity(changes.len());
    for &change in changes {
        let changeset = build_changeset(ctx, repo, source, depot_root, parent, change)
            .await
            .with_context(|| format!("building changelist {change}"))?;
        parent = Some(changeset.get_changeset_id());
        changesets.push((change, changeset));
    }
    Ok(changesets)
}

/// Imports `changes`, in order, onto `bookmark` (created by the first import), deriving
/// the types in `derive` that the repo has enabled, and returns their changesets.
///
/// A changelist has no parent field, unlike a git commit: its parent is whatever the
/// bookmark points at, so the bookmark is the only record of where the import is. That is
/// why the bookmark and the mapping move in one transaction, with a compare-and-swap: if
/// they disagreed, or another import had moved the bookmark, the next changelist would get
/// the wrong parent and every later commit would build on the wrong history.
pub async fn import_changelists(
    ctx: &CoreContext,
    repo: &(
         impl BookmarksRef
         + BonsaiP4MappingArc
         + CommitGraphRef
         + CommitGraphWriterRef
         + FilestoreConfigRef
         + RepoBlobstoreRef
         + RepoDerivedDataRef
         + RepoIdentityRef
         + Send
         + Sync
     ),
    source: &impl ChangeSource,
    depot_root: &str,
    bookmark: &BookmarkKey,
    changes: &[u64],
    derive: &[DerivableType],
) -> Result<Vec<(u64, ChangesetId)>> {
    let Some(&last) = changes.last() else {
        return Ok(Vec::new());
    };
    let old = repo
        .bookmarks()
        .get(ctx.clone(), bookmark, Freshness::MostRecent)
        .await?;
    let (imported, changesets): (Vec<_>, Vec<_>) =
        build_changesets(ctx, repo, source, depot_root, old, changes)
            .await?
            .into_iter()
            .map(|(change, changeset)| ((change, changeset.get_changeset_id()), changeset))
            .unzip();
    let derive = BackfillDerivation::OnlySpecificTypes(derive.to_vec())
        .types(&repo.repo_derived_data().active_config().types);
    save_and_derive_batch(ctx, repo, changesets, &derive).await?;

    let &(_, new) = imported.last().context("built no changesets")?;
    let mut txn = repo.bookmarks().create_transaction(ctx.clone());
    match old {
        None => txn.create(bookmark, new, BookmarkUpdateReason::Blobimport)?,
        Some(old) => txn.update(bookmark, new, old, BookmarkUpdateReason::Blobimport)?,
    }
    let mapping = mapping_txn_hook(repo.bonsai_p4_mapping_arc(), &imported);
    // Hooks run before the bookmark's compare-and-swap, so a concurrent import of the same
    // changelists fails as a mapping `Conflict` rather than here. Both roll back everything.
    ensure!(
        txn.commit_with_hooks(vec![mapping]).await?.is_some(),
        "{bookmark} moved while importing changelists {}..={last}",
        changes[0]
    );
    Ok(imported)
}

/// Writes the batch's mapping in the bookmark move's transaction, so the bookmark never
/// points at a changeset whose changelist isn't mapped, nor the reverse.
fn mapping_txn_hook(
    mapping: Arc<dyn BonsaiP4Mapping>,
    imported: &[(u64, ChangesetId)],
) -> BookmarkTransactionHook {
    let entries: Arc<[_]> = imported
        .iter()
        .map(|&(change, cs_id)| BonsaiP4MappingEntry::new(cs_id, P4ChangelistId::new(change)))
        .collect();
    Arc::new(move |ctx, txn| {
        let mapping = mapping.clone();
        let entries = entries.clone();
        async move {
            mapping
                .bulk_import_in_transaction(&ctx, &entries, txn)
                .await
                .map_err(|error| BookmarkTransactionError::Other(error.into()))
        }
        .boxed()
    })
}

#[cfg(test)]
mod tests;
