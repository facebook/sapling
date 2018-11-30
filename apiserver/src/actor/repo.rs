// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::convert::TryInto;
use std::sync::Arc;

use bytes::Bytes;
use failure::{err_msg, Error};
use futures::{Future, IntoFuture};
use futures::future::join_all;
use futures::sync::oneshot;
use futures_ext::{BoxFuture, FutureExt};
use http::uri::Uri;
use slog::Logger;
use sql::myrouter;
use tokio::runtime::TaskExecutor;

use api;
use blobrepo::{BlobRepo, get_sha256_alias, get_sha256_alias_key};
use context::CoreContext;
use mercurial_types::{HgManifestId, RepositoryId};
use mercurial_types::manifest::Content;
use metaconfig::repoconfig::RepoConfig;
use metaconfig::repoconfig::RepoType::{BlobFiles, BlobManifold, BlobRocks};
use mononoke_types::FileContents;
use reachabilityindex::{GenerationNumberBFS, ReachabilityIndex};

use errors::ErrorKind;
use from_string as FS;

use super::{MononokeRepoQuery, MononokeRepoResponse};
use super::lfs::{build_response, BatchRequest};
use super::model::{Entry, EntryWithSizeAndContentHash};

pub struct MononokeRepo {
    repo: Arc<BlobRepo>,
    logger: Logger,
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
            BlobManifold(args) => match myrouter_port {
                None => Err(err_msg(
                    "Missing myrouter port, unable to open BlobManifold repo",
                )).into_future()
                    .left_future(),
                Some(myrouter_port) => myrouter::wait_for_myrouter(myrouter_port, &args.db_address)
                    .and_then({
                        cloned!(logger);
                        move |()| {
                            BlobRepo::new_manifold_no_postcommit(
                                logger,
                                &args,
                                repoid,
                                myrouter_port,
                            )
                        }
                    })
                    .right_future(),
            },
            _ => Err(err_msg("Unsupported repo type."))
                .into_future()
                .left_future(),
        };

        repo.map(|repo| Self {
            repo: Arc::new(repo),
            logger: logger,
            executor: executor,
        })
    }

    fn get_raw_file(
        &self,
        changeset: String,
        path: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        debug!(
            self.logger,
            "Retrieving file content of {} at changeset {}.", path, changeset
        );

        let mpath = try_boxfuture!(FS::get_mpath(path.clone()));
        let changesetid = try_boxfuture!(FS::get_changeset_id(changeset));
        let repo = self.repo.clone();

        api::get_content_by_path(repo, changesetid, Some(mpath))
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
                cloned!(self.repo);
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
                            .query_reachability(repo.get_changeset_fetcher(), src, dst)
                            .from_err()
                    }
                })
                .then(|r| tx.send(r).map_err(|_| ())),
        );

        rx.flatten()
            .map(|answer| MononokeRepoResponse::IsAncestor { answer })
            .boxify()
    }

    fn get_blob_content(&self, hash: String) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let blobhash = try_boxfuture!(FS::get_nodehash(&hash));

        self.repo
            .get_file_content(&blobhash)
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
        changeset: String,
        path: String,
    ) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let mpath = if path.is_empty() {
            None
        } else {
            Some(try_boxfuture!(FS::get_mpath(path.clone())))
        };
        let changesetid = try_boxfuture!(FS::get_changeset_id(changeset));
        let repo = self.repo.clone();

        api::get_content_by_path(repo, changesetid, mpath)
            .and_then(move |content| match content {
                Content::Tree(tree) => Ok(tree),
                _ => Err(ErrorKind::InvalidInput(path.to_string(), None).into()),
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

    fn get_tree(&self, hash: String) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let treehash = try_boxfuture!(FS::get_nodehash(&hash));
        let treemanifestid = HgManifestId::new(treehash);
        self.repo
            .get_manifest_by_nodeid(&treemanifestid)
            .map(|tree| {
                join_all(
                    tree.list()
                        .map(|entry| EntryWithSizeAndContentHash::materialize_future(entry)),
                )
            })
            .flatten()
            .map(|files| MononokeRepoResponse::GetTree { files })
            .from_err()
            .boxify()
    }

    fn get_changeset(&self, hash: String) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let changesetid = try_boxfuture!(FS::get_changeset_id(hash));

        self.repo
            .get_changeset_by_changesetid(&changesetid)
            .and_then(|changeset| changeset.try_into().map_err(From::from))
            .map(|changeset| MononokeRepoResponse::GetChangeset { changeset })
            .from_err()
            .boxify()
    }

    fn download_large_file(&self, oid: String) -> BoxFuture<MononokeRepoResponse, ErrorKind> {
        let sha256_oid = try_boxfuture!(FS::get_sha256_oid(oid));

        self.repo
            .get_file_content_by_alias(sha256_oid)
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
            .upload_file_content_by_alias(sha256_oid, body)
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
        let lfs_address = try_boxfuture!(
            lfs_url.ok_or(ErrorKind::InvalidInput(
                "Lfs batch request is not allowed, host address is missing in HttpRequest header"
                    .to_string(),
                None
            ))
        );

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
            GetRawFile { changeset, path } => self.get_raw_file(changeset, path),
            GetBlobContent { hash } => self.get_blob_content(hash),
            ListDirectory { changeset, path } => self.list_directory(changeset, path),
            GetTree { hash } => self.get_tree(hash),
            GetChangeset { hash } => self.get_changeset(hash),
            IsAncestor {
                proposed_ancestor,
                proposed_descendent,
            } => self.is_ancestor(ctx, proposed_ancestor, proposed_descendent),

            DownloadLargeFile { oid } => self.download_large_file(oid),
            LfsBatch {
                repo_name,
                req,
                lfs_url,
            } => self.lfs_batch(repo_name, req, lfs_url),
            UploadLargeFile { oid, body } => self.upload_large_file(oid, body),
        }
    }
}
