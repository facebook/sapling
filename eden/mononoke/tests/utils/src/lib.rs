/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{format_err, Error};
use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobrepo_hg::BlobRepoHg;
use blobstore::{Loadable, Storable};
use bookmarks::{BookmarkName, BookmarkUpdateReason};
use bytes::{Bytes, BytesMut};
use context::CoreContext;
use filestore::{self, FetchKey, StoreRequest};
use futures::{
    future,
    stream::{self, TryStreamExt},
};
use manifest::ManifestOps;
use maplit::btreemap;
use mercurial_types::HgChangesetId;
use mononoke_types::{
    BlobstoreValue, BonsaiChangesetMut, ChangesetId, DateTime, FileChange, FileContents, FileType,
    MPath,
};
use std::{
    collections::{BTreeMap, HashMap},
    str::FromStr,
};

pub mod drawdag;

pub async fn list_working_copy_utf8(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<HashMap<MPath, String>, Error> {
    let wc = list_working_copy(ctx, repo, cs_id).await?;

    wc.into_iter()
        .map(|(path, content)| Ok((path, String::from_utf8(content.to_vec())?)))
        .collect()
}

pub async fn list_working_copy_utf8_with_types(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<HashMap<MPath, (String, FileType)>, Error> {
    let wc = list_working_copy_with_types(ctx, repo, cs_id).await?;

    wc.into_iter()
        .map(|(path, (content, ty))| Ok((path, (String::from_utf8(content.to_vec())?, ty))))
        .collect()
}

pub async fn list_working_copy(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<HashMap<MPath, Bytes>, Error> {
    let wc = list_working_copy_with_types(ctx, repo, cs_id).await?;

    Ok(wc
        .into_iter()
        .map(|(path, (bytes, _ty))| (path, bytes))
        .collect())
}

pub async fn list_working_copy_with_types(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<HashMap<MPath, (Bytes, FileType)>, Error> {
    let hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
        .await?;
    let hg_cs = hg_cs_id.load(ctx, repo.blobstore()).await?;

    let mf_id = hg_cs.manifestid();
    mf_id
        .list_leaf_entries(ctx.clone(), repo.blobstore().boxed())
        .map_ok(|(path, (file_type, filenode_id))| async move {
            let filenode = filenode_id.load(ctx, repo.blobstore()).await?;
            let content_id = filenode.content_id();
            let maybe_content = filestore::fetch(
                repo.blobstore(),
                ctx.clone(),
                &FetchKey::Canonical(content_id),
            )
            .await?;
            let s = match maybe_content {
                Some(s) => s,
                None => {
                    return Err(format_err!(
                        "cannot fetch content for {} {}",
                        path,
                        content_id
                    ));
                }
            };
            let bytes = s
                .try_fold(BytesMut::new(), |mut bytes, new_bytes| {
                    bytes.extend_from_slice(&new_bytes);
                    future::ready(Ok(bytes))
                })
                .await?;
            Ok((path, (bytes.freeze(), file_type)))
        })
        .try_buffer_unordered(100)
        .try_collect()
        .await
}

/// Helper to create bonsai changesets in a BlobRepo
pub struct CreateCommitContext<'a> {
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    parents: Vec<CommitIdentifier>,
    files: BTreeMap<MPath, CreateFileContext>,
    message: Option<String>,
    author: Option<String>,
    author_date: Option<DateTime>,
    extra: BTreeMap<String, Vec<u8>>,
}

impl<'a> CreateCommitContext<'a> {
    pub fn new(
        ctx: &'a CoreContext,
        repo: &'a BlobRepo,
        parents: Vec<impl Into<CommitIdentifier>>,
    ) -> Self {
        let parents: Vec<_> = parents.into_iter().map(|x| x.into()).collect();
        Self {
            ctx,
            repo,
            parents,
            files: BTreeMap::new(),
            message: None,
            author: None,
            author_date: None,
            extra: btreemap! {},
        }
    }

    /// Creates commit with no parents (this is created to avoid specifying generic parameters
    /// in CreateCommitContext::new() when `parents` parameter is an empty vector)
    pub fn new_root(ctx: &'a CoreContext, repo: &'a BlobRepo) -> Self {
        Self {
            ctx,
            repo,
            parents: vec![],
            files: BTreeMap::new(),
            message: None,
            author: None,
            author_date: None,
            extra: btreemap! {},
        }
    }

    pub fn add_parent(mut self, id: impl Into<CommitIdentifier>) -> Self {
        self.parents.push(id.into());
        self
    }

    pub fn add_extra(mut self, key: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        self.extra.insert(key.into(), value.into());
        self
    }

    pub fn add_file(mut self, path: impl TryInto<MPath>, content: impl Into<Vec<u8>>) -> Self {
        self.files.insert(
            path.try_into().ok().expect("Invalid path"),
            CreateFileContext::FromHelper(content.into(), FileType::Regular, None),
        );
        self
    }

    pub fn add_files<P: TryInto<MPath>, C: Into<Vec<u8>>, I: IntoIterator<Item = (P, C)>>(
        mut self,
        path_contents: I,
    ) -> Self {
        for (path, content) in path_contents {
            self = self.add_file(path, content);
        }
        self
    }

    pub fn delete_file(mut self, path: impl TryInto<MPath>) -> Self {
        self.files.insert(
            path.try_into().ok().expect("Invalid path"),
            CreateFileContext::Deleted,
        );
        self
    }

    pub fn forget_file(mut self, path: impl TryInto<MPath>) -> Self {
        let path = path.try_into().ok().expect("Invalid path");
        self.files.remove(&path);
        self
    }

    pub fn add_file_with_type(
        mut self,
        path: impl TryInto<MPath>,
        content: impl Into<Vec<u8>>,
        t: FileType,
    ) -> Self {
        self.files.insert(
            path.try_into().ok().expect("Invalid path"),
            CreateFileContext::FromHelper(content.into(), t, None),
        );
        self
    }

    pub fn add_file_with_copy_info(
        mut self,
        path: impl TryInto<MPath>,
        content: impl Into<Vec<u8>>,
        (parent, parent_path): (impl Into<CommitIdentifier>, impl TryInto<MPath>),
    ) -> Self {
        let copy_info = (
            parent_path.try_into().ok().expect("Invalid path"),
            parent.into(),
        );
        self.files.insert(
            path.try_into().ok().expect("Invalid path"),
            CreateFileContext::FromHelper(content.into(), FileType::Regular, Some(copy_info)),
        );
        self
    }

    pub fn add_file_change(mut self, path: impl TryInto<MPath>, file_change: FileChange) -> Self {
        self.files.insert(
            path.try_into().ok().expect("Invalid path"),
            CreateFileContext::FromFileChange(file_change),
        );
        self
    }

    pub fn set_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }

    pub fn set_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    pub fn set_author_date(mut self, author_date: DateTime) -> Self {
        self.author_date = Some(author_date);
        self
    }

    pub async fn create_commit_object(self) -> Result<BonsaiChangesetMut, Error> {
        let parents = future::try_join_all(self.parents.into_iter().map({
            let ctx = &self.ctx;
            let repo = &self.repo;
            move |p| resolve_cs_id(&ctx, &repo, p)
        }))
        .await?;

        let files = future::try_join_all(self.files.into_iter().map({
            let ctx = &self.ctx;
            let repo = &self.repo;
            let parents = &parents;
            move |(path, create_file_context)| async move {
                let file_change = create_file_context
                    .into_file_change(&ctx, &repo, &parents)
                    .await?;

                Result::<_, Error>::Ok((path, file_change))
            }
        }))
        .await?;

        let author_date = match self.author_date {
            Some(author_date) => author_date,
            None => DateTime::from_timestamp(0, 0)?,
        };

        let mut bcs = BonsaiChangesetMut {
            parents,
            author: self.author.unwrap_or_else(|| String::from("author")),
            author_date,
            committer: None,
            committer_date: None,
            message: self.message.unwrap_or_else(|| String::from("message")),
            extra: self.extra.into(),
            file_changes: Default::default(),
            is_snapshot: false,
        };

        for (path, file_change) in files {
            bcs.file_changes.insert(path, file_change);
        }

        Ok(bcs)
    }

    pub async fn commit(self) -> Result<ChangesetId, Error> {
        let ctx = self.ctx.clone();
        let repo = self.repo.clone();
        let bcs = self.create_commit_object().await?;
        let bcs = bcs.freeze()?;

        let bcs_id = bcs.get_changeset_id();
        save_bonsai_changesets(vec![bcs], ctx, &repo).await?;
        Ok(bcs_id)
    }
}

enum CreateFileContext {
    FromHelper(Vec<u8>, FileType, Option<(MPath, CommitIdentifier)>),
    FromFileChange(FileChange),
    Deleted,
}

impl CreateFileContext {
    async fn into_file_change(
        self,
        ctx: &CoreContext,
        repo: &BlobRepo,
        parents: &[ChangesetId],
    ) -> Result<FileChange, Error> {
        let file_change = match self {
            Self::FromHelper(content, file_type, copy_info) => {
                let content = Bytes::copy_from_slice(content.as_ref());

                let meta = filestore::store(
                    repo.blobstore(),
                    repo.filestore_config(),
                    ctx,
                    &StoreRequest::new(content.len().try_into().unwrap()),
                    stream::once(async move { Ok(content) }),
                )
                .await?;

                let copy_info = match copy_info {
                    Some((path, cs_id)) => {
                        let cs_id = resolve_cs_id(&ctx, &repo, cs_id).await?;

                        if !parents.contains(&cs_id) {
                            return Err(format_err!(
                                "CopyInfo at {:?} references invalid parent: {:?}",
                                &path,
                                &cs_id
                            ));
                        }

                        Some((path, cs_id))
                    }
                    None => None,
                };

                FileChange::tracked(meta.content_id, file_type, meta.total_size, copy_info)
            }
            Self::FromFileChange(file_change) => file_change,
            Self::Deleted => FileChange::Deletion,
        };

        Ok(file_change)
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
    pub async fn set_to(
        self,
        cs_ident: impl Into<CommitIdentifier>,
    ) -> Result<BookmarkName, Error> {
        use BookmarkIdentifier::*;
        let bookmark = match self.book_ident {
            Bookmark(bookmark) => bookmark,
            String(s) => BookmarkName::new(s)?,
        };

        let cs_id = resolve_cs_id(&self.ctx, &self.repo, cs_ident).await?;
        let mut book_txn = self.repo.update_bookmark_transaction(self.ctx);
        book_txn.force_set(&bookmark, cs_id, BookmarkUpdateReason::TestMove, None)?;
        book_txn.commit().await?;
        Ok(bookmark)
    }

    pub async fn create_publishing(
        self,
        cs_ident: impl Into<CommitIdentifier>,
    ) -> Result<BookmarkName, Error> {
        use BookmarkIdentifier::*;
        let bookmark = match self.book_ident {
            Bookmark(bookmark) => bookmark,
            String(s) => BookmarkName::new(s)?,
        };

        let cs_id = resolve_cs_id(&self.ctx, &self.repo, cs_ident).await?;
        let mut book_txn = self.repo.update_bookmark_transaction(self.ctx);
        book_txn.create_publishing(&bookmark, cs_id, BookmarkUpdateReason::TestMove, None)?;
        book_txn.commit().await?;
        Ok(bookmark)
    }


    pub async fn create_pull_default(
        self,
        cs_ident: impl Into<CommitIdentifier>,
    ) -> Result<BookmarkName, Error> {
        use BookmarkIdentifier::*;
        let bookmark = match self.book_ident {
            Bookmark(bookmark) => bookmark,
            String(s) => BookmarkName::new(s)?,
        };

        let cs_id = resolve_cs_id(&self.ctx, &self.repo, cs_ident).await?;
        let mut book_txn = self.repo.update_bookmark_transaction(self.ctx);
        book_txn.create(&bookmark, cs_id, BookmarkUpdateReason::TestMove, None)?;
        book_txn.commit().await?;
        Ok(bookmark)
    }

    pub async fn create_scratch(
        self,
        cs_ident: impl Into<CommitIdentifier>,
    ) -> Result<BookmarkName, Error> {
        use BookmarkIdentifier::*;
        let bookmark = match self.book_ident {
            Bookmark(bookmark) => bookmark,
            String(s) => BookmarkName::new(s)?,
        };

        let cs_id = resolve_cs_id(&self.ctx, &self.repo, cs_ident).await?;
        let mut book_txn = self.repo.update_bookmark_transaction(self.ctx);
        book_txn.create_scratch(&bookmark, cs_id)?;
        book_txn.commit().await?;
        Ok(bookmark)
    }

    pub async fn delete(self) -> Result<(), Error> {
        use BookmarkIdentifier::*;
        let bookmark = match self.book_ident {
            Bookmark(bookmark) => bookmark,
            String(s) => BookmarkName::new(s)?,
        };

        let mut book_txn = self.repo.update_bookmark_transaction(self.ctx);
        book_txn.force_delete(&bookmark, BookmarkUpdateReason::TestMove, None)?;
        book_txn.commit().await?;
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

impl From<&BookmarkName> for CommitIdentifier {
    fn from(bookmark: &BookmarkName) -> Self {
        Self::Bookmark(bookmark.clone())
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

impl From<&BookmarkName> for BookmarkIdentifier {
    fn from(bookmark: &BookmarkName) -> Self {
        Self::Bookmark(bookmark.clone())
    }
}

impl From<BookmarkName> for BookmarkIdentifier {
    fn from(bookmark: BookmarkName) -> Self {
        Self::Bookmark(bookmark)
    }
}

pub async fn store_files<T: AsRef<str>>(
    ctx: &CoreContext,
    files: BTreeMap<&str, Option<T>>,
    repo: &BlobRepo,
) -> BTreeMap<MPath, FileChange> {
    let mut res = btreemap! {};

    for (path, content) in files {
        let path = MPath::new(path).unwrap();
        match content {
            Some(content) => {
                let content = content.as_ref();
                let size = content.len();
                let content = FileContents::new_bytes(Bytes::copy_from_slice(content.as_bytes()));
                let content_id = content
                    .into_blob()
                    .store(ctx, repo.blobstore())
                    .await
                    .unwrap();

                let file_change =
                    FileChange::tracked(content_id, FileType::Regular, size as u64, None);
                res.insert(path, file_change);
            }
            None => {
                res.insert(path, FileChange::Deletion);
            }
        }
    }
    res
}

pub async fn store_rename(
    ctx: &CoreContext,
    copy_src: (MPath, ChangesetId),
    path: &str,
    content: &str,
    repo: &BlobRepo,
) -> (MPath, FileChange) {
    let path = MPath::new(path).unwrap();
    let size = content.len();
    let content = FileContents::new_bytes(Bytes::copy_from_slice(content.as_bytes()));
    let content_id = content
        .into_blob()
        .store(ctx, repo.blobstore())
        .await
        .unwrap();

    let file_change =
        FileChange::tracked(content_id, FileType::Regular, size as u64, Some(copy_src));
    (path, file_change)
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
                .bonsai_hg_mapping()
                .get_bonsai_from_hg(ctx, hg_cs_id)
                .await?;
            maybe_cs_id.ok_or(format_err!("{} not found", hg_cs_id))
        }
        Bookmark(bookmark) => {
            let maybe_cs_id = repo.get_bonsai_bookmark(ctx.clone(), &bookmark).await?;
            maybe_cs_id.ok_or(format_err!("{} not found", bookmark))
        }
        String(hash_or_bookmark) => {
            if let Ok(name) = BookmarkName::new(hash_or_bookmark.clone()) {
                if let Ok(Some(csid)) = repo.get_bonsai_bookmark(ctx.clone(), &name).await {
                    return Ok(csid);
                }
            }

            if let Ok(hg_cs_id) = HgChangesetId::from_str(&hash_or_bookmark) {
                if let Ok(Some(cs_id)) = repo
                    .bonsai_hg_mapping()
                    .get_bonsai_from_hg(ctx, hg_cs_id)
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

pub async fn create_commit(
    ctx: CoreContext,
    repo: BlobRepo,
    parents: Vec<ChangesetId>,
    file_changes: BTreeMap<MPath, FileChange>,
) -> ChangesetId {
    let bcs = BonsaiChangesetMut {
        parents,
        author: "author".to_string(),
        author_date: DateTime::from_timestamp(0, 0).unwrap(),
        committer: None,
        committer_date: None,
        message: "message".to_string(),
        extra: Default::default(),
        file_changes: file_changes.into(),
        is_snapshot: false,
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx, &repo).await.unwrap();
    bcs_id
}

pub async fn create_commit_with_date(
    ctx: CoreContext,
    repo: BlobRepo,
    parents: Vec<ChangesetId>,
    file_changes: BTreeMap<MPath, FileChange>,
    author_date: DateTime,
) -> ChangesetId {
    let bcs = BonsaiChangesetMut {
        parents,
        author: "author".to_string(),
        author_date,
        committer: None,
        committer_date: None,
        message: "message".to_string(),
        extra: Default::default(),
        file_changes: file_changes.into(),
        is_snapshot: false,
    }
    .freeze()
    .unwrap();

    let bcs_id = bcs.get_changeset_id();
    save_bonsai_changesets(vec![bcs], ctx, &repo).await.unwrap();
    bcs_id
}
