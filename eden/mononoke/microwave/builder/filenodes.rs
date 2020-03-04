/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use cloned::cloned;
use context::CoreContext;
use filenodes::{FilenodeInfo, Filenodes, PreparedFilenode};
use futures::{
    channel::mpsc::Sender,
    compat::Future01CompatExt,
    future::{FutureExt as _, TryFutureExt},
    sink::SinkExt,
};
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::HgFileNodeId;
use mononoke_types::{RepoPath, RepositoryId};
use std::sync::Arc;

#[derive(Clone)]
pub struct MicrowaveFilenodes {
    repo_id: RepositoryId,
    recorder: Sender<PreparedFilenode>,
    inner: Arc<dyn Filenodes>,
}

impl MicrowaveFilenodes {
    pub fn new(
        repo_id: RepositoryId,
        recorder: Sender<PreparedFilenode>,
        inner: Arc<dyn Filenodes>,
    ) -> Self {
        Self {
            repo_id,
            recorder,
            inner,
        }
    }
}

impl Filenodes for MicrowaveFilenodes {
    fn add_filenodes(
        &self,
        ctx: CoreContext,
        info: Vec<PreparedFilenode>,
        repo_id: RepositoryId,
    ) -> BoxFuture<(), Error> {
        self.inner.add_filenodes(ctx, info, repo_id)
    }

    fn add_or_replace_filenodes(
        &self,
        ctx: CoreContext,
        info: Vec<PreparedFilenode>,
        repo_id: RepositoryId,
    ) -> BoxFuture<(), Error> {
        self.inner.add_or_replace_filenodes(ctx, info, repo_id)
    }

    fn get_filenode(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        filenode_id: HgFileNodeId,
        repo_id: RepositoryId,
    ) -> BoxFuture<Option<FilenodeInfo>, Error> {
        cloned!(self.inner, mut self.recorder, path);

        // NOTE: Receiving any other repo_id here would be a programming error, so we block it.
        // This wouldn't be on the path of any live traffic, so panicking if this assertion is
        // violated is reasonable.
        assert_eq!(repo_id, self.repo_id);

        async move {
            let info = inner
                .get_filenode(ctx, &path, filenode_id, repo_id)
                .compat()
                .await?;

            if let Some(ref info) = info {
                recorder
                    .send(PreparedFilenode {
                        path,
                        info: info.clone(),
                    })
                    .await?;
            }

            Ok(info)
        }
        .boxed()
        .compat()
        .boxify()
    }

    fn get_all_filenodes_maybe_stale(
        &self,
        ctx: CoreContext,
        path: &RepoPath,
        repo_id: RepositoryId,
    ) -> BoxFuture<Vec<FilenodeInfo>, Error> {
        self.inner.get_all_filenodes_maybe_stale(ctx, path, repo_id)
    }

    fn prime_cache(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        filenodes: &[PreparedFilenode],
    ) {
        self.inner.prime_cache(ctx, repo_id, filenodes)
    }
}
