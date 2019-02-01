// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::convert::TryInto;

use bytes::Bytes;
use failure::{err_msg, Error};
use futures::future::join_all;
use futures::sync::oneshot;
use futures::Stream;
use futures::{Future, IntoFuture};
use futures_ext::{BoxFuture, FutureExt};
use http::uri::Uri;
use slog::Logger;
use sql::myrouter;
use tokio::runtime::TaskExecutor;

use api;
use blobrepo::{get_sha256_alias, get_sha256_alias_key, BlobRepo};
use bookmarks::Bookmark;
use context::CoreContext;
use mercurial_types::manifest::Content;

use mercurial_types::{HgChangesetId, HgManifestId};
use metaconfig_types::RepoConfig;
use metaconfig_types::RepoType::{BlobFiles, BlobRemote, BlobRocks, BlobSqlite};

use genbfs::GenerationNumberBFS;
use mononoke_types::{FileContents, RepositoryId};
use reachabilityindex::ReachabilityIndex;

use errors::ErrorKind;
use from_string as FS;

use super::lfs::{build_response, BatchRequest};
use super::model::{Entry, EntryWithSizeAndContentHash};
use super::{MononokeRepoQuery, MononokeRepoResponse, Revision};

pub struct MononokeRepo {
    repo: BlobRepo,
    executor: TaskExecutor,
}

impl MononokeRepo {
    pub fn new(
        logger: Logger,
        config: RepoConfig,
        myrouter_port: Option<u16>,
        executor: TaskExecutor,
    ) -> impl Future<Item = Self, Error = Error> {
        let repoid = RepositoryId::new(config.repoid);
        let repo = match config.repotype {
            BlobFiles(path) => BlobRepo::new_files(logger.clone(), &path, repoid)
                .into_future()
                .left_future(),
            BlobRocks(path) => BlobRepo::new_rocksdb(logger.clone(), &path, repoid)
                .into_future()
                .left_future(),
            BlobSqlite(path) => BlobRepo::new_sqlite(logger.clone(), &path, repoid)
                .into_future()
                .left_future(),
            BlobRemote {
                ref blobstores_args,
                ref db_address,
                ref filenode_shards,
            } => match myrouter_port {
                None => Err(err_msg(
                    "Missing myrouter port, unable to open BlobRemote repo",
                ))
                .into_future()
                .left_future(),
                Some(myrouter_port) => myrouter::wait_for_myrouter(myrouter_port, &db_address)
                    .and_then({
                        cloned!(db_address, filenode_shards, logger, blobstores_args);
                        move |()| {
                            BlobRepo::new_remote_no_postcommit(
                                logger,
                                &blobstores_args,
                                db_address.clone(),
                                filenode_shards.clone(),
                                repoid,
                                myrouter_port,
                            )
                        }
                    })
                    .right_future(),
            },
        };

        repo.map(|repo| Self { repo, executor })
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
            .get_raw_hg_content(ctx, &filenode)
            .map(|content| MononokeRepoResponse::GetHgFile {
                content: content.into_inner(),
            })
            .from_err()
            .boxify()
    }

    fn is_ancestor(
        &self,
        ctx: CoreContext,
        proposed_ancestor: String,
        proposed_descendent: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let genbfs = GenerationNumberBFS::new();
        let src_hash_maybe = FS::get_changeset_id(proposed_descendent.clone());
        let dst_hash_maybe = FS::get_changeset_id(proposed_ancestor.clone());
        let src_hash_future = src_hash_maybe.into_future().or_else({
            cloned!(ctx, self.repo, proposed_descendent);
            move |_| FS::string_to_bookmark_changeset_id(ctx, proposed_descendent, repo)
        });

        let src_hash_future = src_hash_future
            .and_then({
                cloned!(ctx, self.repo);
                move |hg_cs_id| repo.get_bonsai_from_hg(ctx, &hg_cs_id).from_err()
            })
            .and_then(move |maybenode| {
                maybenode.ok_or(ErrorKind::NotFound(
                    format!("{}", proposed_descendent),
                    None,
                ))
            });

        let dst_hash_future = dst_hash_maybe.into_future().or_else({
            cloned!(ctx, self.repo, proposed_ancestor);
            move |_| FS::string_to_bookmark_changeset_id(ctx, proposed_ancestor, repo)
        });

        let dst_hash_future = dst_hash_future
            .and_then({
                cloned!(ctx, self.repo);
                move |hg_cs_id| repo.get_bonsai_from_hg(ctx, &hg_cs_id).from_err()
            })
            .and_then(move |maybenode| {
                maybenode.ok_or(ErrorKind::NotFound(format!("{}", proposed_ancestor), None))
            });

        let (tx, rx) = oneshot::channel::<Result<bool, ErrorKind>>();

        self.executor.spawn(
            src_hash_future
                .and_then(|src| dst_hash_future.map(move |dst| (src, dst)))
                .and_then({
                    cloned!(self.repo);
                    move |(src, dst)| {
                        genbfs
                            .query_reachability(ctx, repo.get_changeset_fetcher(), src, dst)
                            .from_err()
                    }
                })
                .then(|r| tx.send(r).map_err(|_| ())),
        );

        rx.flatten()
            .map(|answer| MononokeRepoResponse::IsAncestor { answer })
            .boxify()
    }

    fn get_blob_content(
        &self,
        ctx: CoreContext,
        hash: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let blobhash = try_boxfuture!(FS::get_nodehash(&hash));

        self.repo
            .get_file_content(ctx, &blobhash)
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
            .get_manifest_by_nodeid(ctx.clone(), &treemanifestid)
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
            .and_then(move |changesetid| repo.get_changeset_by_changesetid(ctx, &changesetid))
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
        let lfs_address = try_boxfuture!(lfs_url.ok_or(ErrorKind::InvalidInput(
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
                proposed_ancestor,
                proposed_descendent,
            } => self.is_ancestor(ctx, proposed_ancestor, proposed_descendent),

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
