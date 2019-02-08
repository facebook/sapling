// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::HashMap;
use std::convert::TryInto;
use std::sync::Arc;

use api;
use blobrepo::{get_sha256_alias, get_sha256_alias_key, BlobRepo};
use blobrepo_factory::open_blobrepo;
use blobstore::Blobstore;
use bookmarks::Bookmark;
use bytes::Bytes;
use context::CoreContext;
use failure::Error;
use futures::future::{join_all, ok};
use futures::Stream;
use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use http::uri::Uri;
use mercurial_types::manifest::Content;
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use tracing::TraceContext;
use uuid::Uuid;

use mercurial_types::{HgChangesetId, HgFileNodeId, HgManifestId};
use metaconfig_types::RepoConfig;

use mononoke_types::{FileContents, RepositoryId};
use reachabilityindex::ReachabilityIndex;
use skiplist::{deserialize_skiplist_map, SkiplistIndex};

use errors::ErrorKind;
use from_string as FS;

use super::lfs::{build_response, BatchRequest};
use super::model::{Entry, EntryWithSizeAndContentHash};
use super::{MononokeRepoQuery, MononokeRepoResponse, Revision};

pub struct MononokeRepo {
    repo: BlobRepo,
    skiplist_index: Arc<SkiplistIndex>,
}

impl MononokeRepo {
    pub fn new(
        logger: Logger,
        config: RepoConfig,
        myrouter_port: Option<u16>,
    ) -> impl Future<Item = Self, Error = Error> {
        let ctx = CoreContext::new(
            Uuid::new_v4(),
            logger.clone(),
            ScubaSampleBuilder::with_discard(),
            None,
            TraceContext::default(),
        );

        let skiplist_index_blobstore_key = config.skiplist_index_blobstore_key.clone();

        let repoid = RepositoryId::new(config.repoid);
        open_blobrepo(logger.clone(), config.repotype, repoid, myrouter_port)
            .map(move |repo| {
                let skiplist_index = match skiplist_index_blobstore_key.clone() {
                    Some(skiplist_index_blobstore_key) => repo
                        .get_blobstore()
                        .get(ctx.clone(), skiplist_index_blobstore_key)
                        .and_then(|maybebytes| {
                            let map = match maybebytes {
                                Some(bytes) => {
                                    let bytes = bytes.into_bytes();
                                    try_boxfuture!(deserialize_skiplist_map(bytes))
                                }
                                None => HashMap::new(),
                            };
                            ok(Arc::new(SkiplistIndex::new_with_skiplist_graph(map))).boxify()
                        })
                        .left_future(),
                    None => ok(Arc::new(SkiplistIndex::new())).right_future(),
                };
                skiplist_index.map(|skiplist_index| Self {
                    repo,
                    skiplist_index,
                })
            })
            .flatten()
    }

    fn get_hgchangesetid_from_revision(
        &self,
        ctx: CoreContext,
        revision: Revision,
    ) -> impl Future<Item = HgChangesetId, Error = Error> {
        let repo = self.repo.clone();
        match revision {
            Revision::CommitHash(hash) => FS::get_changeset_id(hash)
                .into_future()
                .from_err()
                .left_future(),
            Revision::Bookmark(bookmark) => Bookmark::new(bookmark)
                .into_future()
                .from_err()
                .and_then(move |bookmark| {
                    repo.get_bookmark(ctx, &bookmark).and_then(move |opt| {
                        opt.ok_or_else(|| ErrorKind::BookmarkNotFound(bookmark.to_string()).into())
                    })
                })
                .right_future(),
        }
    }

    fn get_raw_file(
        &self,
        ctx: CoreContext,
        revision: Revision,
        path: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let mpath = try_boxfuture!(FS::get_mpath(path.clone()));

        let repo = self.repo.clone();
        self.get_hgchangesetid_from_revision(ctx.clone(), revision)
            .and_then(|changesetid| api::get_content_by_path(ctx, repo, changesetid, Some(mpath)))
            .and_then(move |content| match content {
                Content::File(content)
                | Content::Executable(content)
                | Content::Symlink(content) => Ok(MononokeRepoResponse::GetRawFile {
                    content: content.into_bytes(),
                }),
                _ => Err(ErrorKind::InvalidInput(path.to_string(), None).into()),
            })
            .from_err()
            .boxify()
    }

    /// Given a Mercurial filenode hash, return the raw content of the file in the format
    /// expected by the Mercurial client. This includes the raw bytes of the file content,
    /// optionally prefixed with a header containing copy-from information. Content in
    /// this format can be directly stored by Mercurial without additional manipulation.
    fn get_hg_file(
        &self,
        ctx: CoreContext,
        filenode: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let filenode = try_boxfuture!(FS::get_nodehash(&filenode));
        self.repo
            .get_raw_hg_content(ctx, HgFileNodeId::new(filenode))
            .map(|content| MononokeRepoResponse::GetHgFile {
                content: content.into_inner(),
            })
            .from_err()
            .boxify()
    }

    fn is_ancestor(
        &self,
        ctx: CoreContext,
        ancestor: Revision,
        descendant: Revision,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let descendant_future = self
            .get_hgchangesetid_from_revision(ctx.clone(), descendant.clone())
            .from_err()
            .and_then({
                cloned!(ctx, self.repo);
                move |hg_cs_id| repo.get_bonsai_from_hg(ctx, hg_cs_id).from_err()
            })
            .and_then(move |maybenode| {
                maybenode.ok_or(ErrorKind::NotFound(format!("{:?}", descendant), None))
            });

        let ancestor_future = self
            .get_hgchangesetid_from_revision(ctx.clone(), ancestor.clone())
            .from_err()
            .and_then({
                cloned!(ctx, self.repo);
                move |hg_cs_id| repo.get_bonsai_from_hg(ctx, hg_cs_id).from_err()
            })
            .and_then(move |maybenode| {
                maybenode.ok_or(ErrorKind::NotFound(format!("{:?}", ancestor), None))
            });

        descendant_future
            .join(ancestor_future)
            .map({
                cloned!(self.repo, self.skiplist_index);
                move |(desc, anc)| {
                    skiplist_index.query_reachability(ctx, repo.get_changeset_fetcher(), desc, anc)
                }
            })
            .flatten()
            .map(|answer| MononokeRepoResponse::IsAncestor { answer })
            .from_err()
            .boxify()
    }

    fn get_blob_content(
        &self,
        ctx: CoreContext,
        hash: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let blobhash = try_boxfuture!(FS::get_nodehash(&hash));

        self.repo
            .get_file_content(ctx, HgFileNodeId::new(blobhash))
            .and_then(move |content| match content {
                FileContents::Bytes(content) => {
                    Ok(MononokeRepoResponse::GetBlobContent { content })
                }
            })
            .from_err()
            .boxify()
    }

    fn list_directory(
        &self,
        ctx: CoreContext,
        revision: Revision,
        path: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let mpath = if path.is_empty() {
            None
        } else {
            Some(try_boxfuture!(FS::get_mpath(path.clone())))
        };

        let repo = self.repo.clone();
        self.get_hgchangesetid_from_revision(ctx.clone(), revision)
            .and_then(move |changesetid| api::get_content_by_path(ctx, repo, changesetid, mpath))
            .and_then(move |content| match content {
                Content::Tree(tree) => Ok(tree),
                _ => Err(ErrorKind::NotADirectory(path.to_string()).into()),
            })
            .map(|tree| {
                tree.list()
                    .filter_map(|entry| -> Option<Entry> { entry.try_into().ok() })
            })
            .map(|files| MononokeRepoResponse::ListDirectory {
                files: Box::new(files),
            })
            .from_err()
            .boxify()
    }

    fn get_tree(
        &self,
        ctx: CoreContext,
        hash: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let treehash = try_boxfuture!(FS::get_nodehash(&hash));
        let treemanifestid = HgManifestId::new(treehash);
        self.repo
            .get_manifest_by_nodeid(ctx.clone(), treemanifestid)
            .map(move |tree| {
                join_all(tree.list().map(move |entry| {
                    EntryWithSizeAndContentHash::materialize_future(ctx.clone(), entry)
                }))
            })
            .flatten()
            .map(|files| MononokeRepoResponse::GetTree { files })
            .from_err()
            .boxify()
    }

    fn get_changeset(
        &self,
        ctx: CoreContext,
        revision: Revision,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let repo = self.repo.clone();
        self.get_hgchangesetid_from_revision(ctx.clone(), revision)
            .and_then(move |changesetid| repo.get_changeset_by_changesetid(ctx, changesetid))
            .and_then(|changeset| changeset.try_into().map_err(From::from))
            .map(|changeset| MononokeRepoResponse::GetChangeset { changeset })
            .from_err()
            .boxify()
    }

    fn get_branches(&self, ctx: CoreContext) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        self.repo
            .get_bookmarks_maybe_stale(ctx)
            .map(|(bookmark, changesetid)| (bookmark.to_string(), changesetid.to_hex().to_string()))
            .collect()
            .map(|vec| MononokeRepoResponse::GetBranches {
                branches: vec.into_iter().collect(),
            })
            .from_err()
            .boxify()
    }

    fn download_large_file(
        &self,
        ctx: CoreContext,
        oid: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let sha256_oid = try_boxfuture!(FS::get_sha256_oid(oid));

        self.repo
            .get_file_content_by_alias(ctx, sha256_oid)
            .and_then(move |content| match content {
                FileContents::Bytes(content) => {
                    Ok(MononokeRepoResponse::DownloadLargeFile { content })
                }
            })
            .from_err()
            .boxify()
    }

    fn upload_large_file(
        &self,
        ctx: CoreContext,
        oid: String,
        body: Bytes,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let sha256_oid = try_boxfuture!(FS::get_sha256_oid(oid.clone()));

        let calculated_sha256_key = get_sha256_alias(&body);
        let given_sha256_key = get_sha256_alias_key(oid);

        if calculated_sha256_key != given_sha256_key {
            try_boxfuture!(Err(ErrorKind::InvalidInput(
                "Upload file content has different sha256".to_string(),
                None,
            )))
        }

        self.repo
            .upload_file_content_by_alias(ctx, sha256_oid, body)
            .and_then(|_| Ok(MononokeRepoResponse::UploadLargeFile {}))
            .from_err()
            .boxify()
    }

    fn lfs_batch(
        &self,
        repo_name: String,
        req: BatchRequest,
        lfs_url: Option<Uri>,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let lfs_address =
            try_boxfuture!(lfs_url.ok_or(ErrorKind::InvalidInput(
            "Lfs batch request is not allowed, host address is missing in HttpRequest header"
                .to_string(),
            None
        )));

        let response = build_response(repo_name, req, lfs_address);
        Ok(MononokeRepoResponse::LfsBatch { response })
            .into_future()
            .boxify()
    }

    pub fn send_query(
        &self,
        ctx: CoreContext,
        msg: MononokeRepoQuery,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        use MononokeRepoQuery::*;

        match msg {
            GetRawFile { revision, path } => self.get_raw_file(ctx, revision, path),
            GetHgFile { filenode } => self.get_hg_file(ctx, filenode),
            GetBlobContent { hash } => self.get_blob_content(ctx, hash),
            ListDirectory { revision, path } => self.list_directory(ctx, revision, path),
            GetTree { hash } => self.get_tree(ctx, hash),
            GetChangeset { revision } => self.get_changeset(ctx, revision),
            GetBranches => self.get_branches(ctx),
            IsAncestor {
                ancestor,
                descendant,
            } => self.is_ancestor(ctx, ancestor, descendant),

            DownloadLargeFile { oid } => self.download_large_file(ctx, oid),
            LfsBatch {
                repo_name,
                req,
                lfs_url,
            } => self.lfs_batch(repo_name, req, lfs_url),
            UploadLargeFile { oid, body } => self.upload_large_file(ctx, oid, body),
        }
    }
}
