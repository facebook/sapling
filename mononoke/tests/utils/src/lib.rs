/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use anyhow::{format_err, Error};
use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobstore::Storable;
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use bytes::Bytes;
use context::CoreContext;
use futures::future::Future;
use futures_util::{compat::Future01CompatExt, future};
use maplit::btreemap;
use mercurial_types::HgChangesetId;
use mononoke_types::{
    BlobstoreValue, BonsaiChangesetMut, ChangesetId, DateTime, FileChange, FileContents, FileType,
    MPath,
};
use std::{collections::BTreeMap, str::FromStr};

/// Helper to create bonsai changesets in a BlobRepo
pub struct CreateCommitContext {
    ctx: CoreContext,
    repo: BlobRepo,
    parents: Vec<CommitIdentifier>,
    files: BTreeMap<String, String>,
}

impl CreateCommitContext {
    pub fn new(
        ctx: CoreContext,
        repo: BlobRepo,
        parents: Vec<impl Into<CommitIdentifier>>,
    ) -> Self {
        let parents: Vec<_> = parents.into_iter().map(|x| x.into()).collect();
        Self {
            ctx,
            repo,
            parents,
            files: BTreeMap::new(),
        }
    }

    /// Creates commit with no parents (this is created to avoid specifying generic parameters
    /// in CreateCommitContext::new() when `parents` parameter is an empty vector)
    pub fn new_root(ctx: CoreContext, repo: BlobRepo) -> Self {
        Self {
            ctx,
            repo,
            parents: vec![],
            files: BTreeMap::new(),
        }
    }

    pub fn add_file(mut self, path: &str, content: impl AsRef<str>) -> Self {
        self.files
            .insert(path.to_string(), content.as_ref().to_string());
        self
    }

    pub async fn commit(self) -> Result<ChangesetId, Error> {
        let ctx = self.ctx.clone();
        let repo = self.repo.clone();
        let parents = future::try_join_all(
            self.parents
                .into_iter()
                .map(|p| resolve_cs_id(&ctx, &repo, p)),
        )
        .await?;
        let mut bcs = BonsaiChangesetMut {
            parents,
            author: "author".to_string(),
            author_date: DateTime::from_timestamp(0, 0)?,
            committer: None,
            committer_date: None,
            message: "message".to_string(),
            extra: btreemap! {},
            file_changes: btreemap! {},
        };

        for (path, content) in self.files {
            let path = MPath::new(path)?;
            let size = content.len();
            let content = FileContents::new_bytes(Bytes::from(content));
            let content_id = content
                .into_blob()
                .store(self.ctx.clone(), self.repo.blobstore())
                .compat()
                .await?;
            let file_change = FileChange::new(content_id, FileType::Regular, size as u64, None);
            bcs.file_changes.insert(path, Some(file_change));
        }

        let bcs = bcs.freeze()?;

        let bcs_id = bcs.get_changeset_id();
        save_bonsai_changesets(vec![bcs], self.ctx, self.repo.clone())
            .compat()
            .await?;
        Ok(bcs_id)
    }
}

/// Returns helper that can be moved to move/delete/create a bookmark
pub fn bookmark(
    ctx: &CoreContext,
    repo: &BlobRepo,
    book_ident: impl Into<BookmarkIdentifier>,
) -> UpdateBookmarkContext {
    UpdateBookmarkContext {
        ctx: ctx.clone(),
        repo: repo.clone(),
        book_ident: book_ident.into(),
    }
}

pub struct UpdateBookmarkContext {
    ctx: CoreContext,
    repo: BlobRepo,
    book_ident: BookmarkIdentifier,
}

impl UpdateBookmarkContext {
    pub async fn set_to(self, cs_ident: impl Into<CommitIdentifier>) -> Result<(), Error> {
        use BookmarkIdentifier::*;
        let bookmark = match self.book_ident {
            Bookmark(bookmark) => bookmark,
            String(s) => BookmarkName::new(s)?,
        };

        let cs_id = resolve_cs_id(&self.ctx, &self.repo, cs_ident).await?;
        let mut book_txn = self.repo.update_bookmark_transaction(self.ctx);
        book_txn.force_set(
            &bookmark,
            cs_id,
            BookmarkUpdateReason::TestMove {
                bundle_replay_data: None,
            },
        )?;
        book_txn.commit().compat().await?;
        Ok(())
    }

    pub async fn delete(self) -> Result<(), Error> {
        use BookmarkIdentifier::*;
        let bookmark = match self.book_ident {
            Bookmark(bookmark) => bookmark,
            String(s) => BookmarkName::new(s)?,
        };

        let mut book_txn = self.repo.update_bookmark_transaction(self.ctx);
        book_txn.force_delete(
            &bookmark,
            BookmarkUpdateReason::TestMove {
                bundle_replay_data: None,
            },
        )?;
        book_txn.commit().compat().await?;
        Ok(())
    }
}

pub enum CommitIdentifier {
    Bonsai(ChangesetId),
    Hg(HgChangesetId),
    String(String),
    Bookmark(BookmarkName),
}

impl From<ChangesetId> for CommitIdentifier {
    fn from(bcs_id: ChangesetId) -> Self {
        Self::Bonsai(bcs_id)
    }
}

impl From<HgChangesetId> for CommitIdentifier {
    fn from(hg_cs_id: HgChangesetId) -> Self {
        Self::Hg(hg_cs_id)
    }
}

impl From<&str> for CommitIdentifier {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl From<String> for CommitIdentifier {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<BookmarkName> for CommitIdentifier {
    fn from(bookmark: BookmarkName) -> Self {
        Self::Bookmark(bookmark)
    }
}

pub enum BookmarkIdentifier {
    String(String),
    Bookmark(BookmarkName),
}

impl From<&str> for BookmarkIdentifier {
    fn from(s: &str) -> Self {
        Self::String(s.to_string())
    }
}

impl From<String> for BookmarkIdentifier {
    fn from(s: String) -> Self {
        Self::String(s)
    }
}

impl From<BookmarkName> for BookmarkIdentifier {
    fn from(bookmark: BookmarkName) -> Self {
        Self::Bookmark(bookmark)
    }
}

pub fn store_files<T: AsRef<str>>(
    ctx: CoreContext,
    files: BTreeMap<&str, Option<T>>,
    repo: BlobRepo,
) -> BTreeMap<MPath, Option<FileChange>> {
    let mut res = btreemap! {};

    for (path, content) in files {
        let path = MPath::new(path).unwrap();
        match content {
            Some(content) => {
                let content = content.as_ref();
                let size = content.len();
                let content = FileContents::new_bytes(Bytes::from(content));
                let content_id = content
                    .into_blob()
                    .store(ctx.clone(), repo.blobstore())
                    .wait()
                    .unwrap();

                let file_change = FileChange::new(content_id, FileType::Regular, size as u64, None);
                res.insert(path, Some(file_change));
            }
            None => {
                res.insert(path, None);
            }
        }
    }
    res
}

pub fn store_rename(
    ctx: CoreContext,
    copy_src: (MPath, ChangesetId),
    path: &str,
    content: &str,
    repo: BlobRepo,
) -> (MPath, Option<FileChange>) {
    let path = MPath::new(path).unwrap();
    let size = content.len();
    let content = FileContents::new_bytes(Bytes::from(content));
    let content_id = content
        .into_blob()
        .store(ctx, repo.blobstore())
        .wait()
        .unwrap();

    let file_change = FileChange::new(content_id, FileType::Regular, size as u64, Some(copy_src));
    (path, Some(file_change))
}

pub async fn resolve_cs_id(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_ident: impl Into<CommitIdentifier>,
) -> Result<ChangesetId, Error> {
    use CommitIdentifier::*;
    match cs_ident.into() {
        Bonsai(cs_id) => Ok(cs_id),
        Hg(hg_cs_id) => {
            let maybe_cs_id = repo
                .get_bonsai_from_hg(ctx.clone(), hg_cs_id)
                .compat()
                .await?;
            maybe_cs_id.ok_or(format_err!("{} not found", hg_cs_id))
        }
        Bookmark(bookmark) => {
            let maybe_cs_id = repo
                .get_bonsai_bookmark(ctx.clone(), &bookmark)
                .compat()
                .await?;
            maybe_cs_id.ok_or(format_err!("{} not found", bookmark))
        }
        String(hash_or_bookmark) => {
            if let Ok(name) = BookmarkName::new(hash_or_bookmark.clone()) {
                if let Ok(Some(csid)) = repo.get_bonsai_bookmark(ctx.clone(), &name).compat().await
                {
                    return Ok(csid);
                }
            }

            if let Ok(hg_cs_id) = HgChangesetId::from_str(&hash_or_bookmark) {
                if let Ok(Some(cs_id)) = repo
                    .get_bonsai_from_hg(ctx.clone(), hg_cs_id)
                    .compat()
                    .await
                {
                    return Ok(cs_id);
                }
            }

            if let Ok(cs_id) = ChangesetId::from_str(&hash_or_bookmark) {
                return Ok(cs_id);
            }
            Err(format_err!(
                "invalid (hash|bookmark) or does not exist in this repository: {}",
                hash_or_bookmark
            ))
        }
    }
}

pub fn create_commit(
    ctx: CoreContext,
    repo: BlobRepo,
    parents: Vec<ChangesetId>,
    file_changes: BTreeMap<MPath, Option<FileChange>>,
) -> ChangesetId {
    let bcs = BonsaiChangesetMut {
        parents,
        author: "author".to_string(),
        author_date: DateTime::from_timestamp(0, 0).unwrap(),
        committer: None,
        committer_date: None,
        message: "message".to_string(),
        extra: btreemap! {},
        file_changes,
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx, repo.clone())
        .wait()
        .unwrap();
    bcs_id
}

pub fn create_commit_with_date(
    ctx: CoreContext,
    repo: BlobRepo,
    parents: Vec<ChangesetId>,
    file_changes: BTreeMap<MPath, Option<FileChange>>,
    author_date: DateTime,
) -> ChangesetId {
    let bcs = BonsaiChangesetMut {
        parents,
        author: "author".to_string(),
        author_date,
        committer: None,
        committer_date: None,
        message: "message".to_string(),
        extra: btreemap! {},
        file_changes,
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx, repo.clone())
        .wait()
        .unwrap();
    bcs_id
}
