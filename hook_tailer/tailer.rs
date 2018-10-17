// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use blobrepo::BlobRepo;
use bookmarks::Bookmark;
use failure::Error;
use failure::Result;
use futures::{Future, Stream};
use futures_ext::{BoxFuture, FutureExt, spawn_future};
use hooks::{BlobRepoChangesetStore, BlobRepoFileContentStore, ChangesetHookExecutionID,
            FileHookExecutionID, HookExecution, HookManager, hook_loader::load_hooks};
use manifold::{ManifoldHttpClient, PayloadRange};
use mercurial_types::{HgChangesetId, HgNodeHash};
use metaconfig::repoconfig::RepoConfig;
use mononoke_types::ChangesetId;
use revset::AncestorsNodeStream;
use slog::Logger;
use std::sync::Arc;

pub struct Tailer {
    repo: Arc<BlobRepo>,
    hook_manager: Arc<HookManager>,
    bookmark: Bookmark,
    last_rev_key: String,
    manifold_client: ManifoldHttpClient,
    logger: Logger,
}

impl Tailer {
    pub fn new(
        repo_name: String,
        repo: Arc<BlobRepo>,
        config: RepoConfig,
        bookmark: Bookmark,
        manifold_client: ManifoldHttpClient,
        logger: Logger,
    ) -> Result<Tailer> {
        let changeset_store = BlobRepoChangesetStore::new((*repo).clone());
        let content_store = BlobRepoFileContentStore::new((*repo).clone());

        let mut hook_manager = HookManager::new(
            repo_name,
            Box::new(changeset_store),
            Arc::new(content_store),
            1024 * 1024, // TODO make configurable T34438181
            1024 * 1024 * 1024,
            logger.clone(),
        );

        load_hooks(&mut hook_manager, config)?;

        let repo_id = repo.get_repoid().id();
        let last_rev_key = format!("{}{}", "__mononoke_hook_tailer_last_rev.", repo_id).to_string();

        Ok(Tailer {
            repo,
            hook_manager: Arc::new(hook_manager),
            bookmark,
            last_rev_key,
            manifold_client,
            logger,
        })
    }

    pub fn get_last_rev_key(&self) -> String {
        self.last_rev_key.clone()
    }

    fn run_in_range0(
        repo: Arc<BlobRepo>,
        hm: Arc<HookManager>,
        last_rev: HgNodeHash,
        end_rev: HgNodeHash,
        bm: Bookmark,
        logger: Logger,
    ) -> BoxFuture<
        Vec<
            (
                Vec<(FileHookExecutionID, HookExecution)>,
                Vec<(ChangesetHookExecutionID, HookExecution)>,
            ),
        >,
        Error,
    > {
        debug!(logger, "Running in range {} to {}", last_rev, end_rev);
        cloned!(logger);
        nodehash_to_bonsai(&repo, end_rev)
            .and_then(move |end_rev| {
                AncestorsNodeStream::new(&repo.get_changeset_fetcher(), end_rev)
                    .take(1000) // Limit number so we don't process too many
                    .map({
                        move |cs| {
                            cloned!(bm, hm, logger, repo);
                            run_hooks_for_changeset(repo, hm, bm, cs, logger)
                        }
                    })
                    .map(spawn_future)
                    .buffered(100)
                    .take_while(move |(hg_cs, _, _)| {
                        Ok(*hg_cs != HgChangesetId::new(last_rev))
                    })
                    .map(|(_, file_res, res)| {
                        (file_res, res)
                    })
                    .collect()
            })
            .boxify()
    }

    pub fn run_in_range(
        &self,
        last_rev: HgNodeHash,
        end_rev: HgNodeHash,
    ) -> BoxFuture<
        Vec<
            (
                Vec<(FileHookExecutionID, HookExecution)>,
                Vec<(ChangesetHookExecutionID, HookExecution)>,
            ),
        >,
        Error,
    > {
        let repo = self.repo.clone();
        let hm = self.hook_manager.clone();
        let bm = self.bookmark.clone();
        Tailer::run_in_range0(repo, hm, last_rev, end_rev, bm, self.logger.clone())
    }

    pub fn run(
        &self,
    ) -> BoxFuture<
        Vec<
            (
                Vec<(FileHookExecutionID, HookExecution)>,
                Vec<(ChangesetHookExecutionID, HookExecution)>,
            ),
        >,
        Error,
    > {
        let bm = self.bookmark.clone();
        let bm2 = bm.clone();
        let repo = self.repo.clone();
        let hm = self.hook_manager.clone();
        let last_rev_key = self.last_rev_key.clone();
        let last_rev_key2 = last_rev_key.clone();
        let manifold_client = self.manifold_client.clone();
        let manifold_client2 = manifold_client.clone();

        info!(self.logger, "Running tailer on bookmark {}", bm);

        let logger = self.logger.clone();
        let logger2 = logger.clone();
        let logger3 = logger.clone();

        self.repo
            .get_bookmark(&bm)
            .and_then(|opt| opt.ok_or(ErrorKind::NoSuchBookmark(bm).into()))
            .and_then(move |current_bm_cs| {
                manifold_client
                    .read(last_rev_key, PayloadRange::Full)
                    .map(move |opt| (current_bm_cs, opt))
            })
            .and_then(|(current_bm_cs, opt)| match opt {
                Some(last_rev_bytes) => Ok((current_bm_cs, last_rev_bytes)),
                None => Err(ErrorKind::NoLastRevision.into()),
            })
            .and_then(|(current_bm_cs, last_rev_bytes)| {
                let node_hash = HgNodeHash::from_bytes(&*last_rev_bytes.payload.payload)?;
                Ok((current_bm_cs, node_hash))
            })
            .and_then(move |(current_bm_cs, last_rev)| {
                let end_rev = current_bm_cs.into_nodehash();
                info!(
                    logger,
                    "Bookmark is currently at {}, last processed revision is {}", end_rev, last_rev
                );
                if last_rev == end_rev {
                    info!(logger, "Nothing to do");
                }
                Tailer::run_in_range0(repo, hm, last_rev, end_rev, bm2, logger3)
                    .map(move |res| (end_rev, res))
            })
            .and_then(move |(end_rev, res)| {
                info!(logger2, "Setting last processed revision to {:?}", end_rev);
                let bytes = end_rev.as_bytes().into();
                manifold_client2.write(last_rev_key2, bytes).map(|()| res)
            })
            .boxify()
    }
}

fn nodehash_to_bonsai(
    repo: &Arc<BlobRepo>,
    node: HgNodeHash,
) -> impl Future<Item = ChangesetId, Error = Error> {
    let hg_cs = HgChangesetId::new(node);
    repo.get_bonsai_from_hg(&hg_cs)
        .and_then(move |maybe_node| maybe_node.ok_or(ErrorKind::BonsaiNotFound(hg_cs).into()))
}

fn run_hooks_for_changeset(
    repo: Arc<BlobRepo>,
    hm: Arc<HookManager>,
    bm: Bookmark,
    cs: ChangesetId,
    logger: Logger,
) -> impl Future<
    Item = (
        HgChangesetId,
        Vec<(FileHookExecutionID, HookExecution)>,
        Vec<(ChangesetHookExecutionID, HookExecution)>,
    ),
    Error = Error,
> {
    repo.get_hg_from_bonsai_changeset(cs)
        .and_then(move |hg_cs| {
            debug!(logger, "Running file hooks for changeset {:?}", hg_cs);
            hm.run_file_hooks_for_bookmark(hg_cs.clone(), &bm, None)
                .map(move |res| (hg_cs, res))
                .and_then(move |(hg_cs, file_res)| {
                    let hg_cs = hg_cs.clone();
                    debug!(logger, "Running changeset hooks for changeset {:?}", hg_cs);
                    hm.run_changeset_hooks_for_bookmark(hg_cs.clone(), &bm, None)
                        .map(move |res| (hg_cs, file_res, res))
                })
        })
}

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "No such bookmark '{}'", _0)] NoSuchBookmark(Bookmark),
    #[fail(display = "Cannot find last revision in blobstore")] NoLastRevision,
    #[fail(display = "Cannot find bonsai for {}", _0)] BonsaiNotFound(HgChangesetId),
}
