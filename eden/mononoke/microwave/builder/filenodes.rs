/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use filenodes::FilenodeInfo;
use filenodes::FilenodeRangeResult;
use filenodes::FilenodeResult;
use filenodes::Filenodes;
use filenodes::PreparedFilenode;
use futures::channel::mpsc::Sender;
use futures::sink::SinkExt;
use mercurial_types::HgFileNodeId;
use mononoke_types::RepoPath;
use std::sync::Arc;

#[derive(Clone)]
pub struct MicrowaveFilenodes {
    recorder: Sender<PreparedFilenode>,
    inner: Arc<dyn Filenodes>,
}

impl MicrowaveFilenodes {
    pub fn new(recorder: Sender<PreparedFilenode>, inner: Arc<dyn Filenodes>) -> Self {
        Self { recorder, inner }
    }
}

#[async_trait]
impl Filenodes for MicrowaveFilenodes {
    async fn add_filenodes(
        &self,
        _ctx: &CoreContext,
        _info: Vec<PreparedFilenode>,
    ) -> Result<FilenodeResult<()>> {
        // Microwave normally should never be writing. If it is writing, then that's likely a bug
        // that warants attention, and it is preferable to panic and wait for a fix. Since
        // Microwave isn't on the critical path for anything, and can do its job perfectly well
        // even if cache building is down for several days, there is little downside. The
        // alternatives are however undesirable:
        //
        // - Simply ignoring the write may result in broken assumptions made by whatever is reading
        // (e.g. if we drop filenodes writes, but we still insert a derived data mappign entry
        // reporting filenodes as derived later, then we'll have missing filenodes in the DB).
        //
        // - Proxying the write to the `inner` implementation should in theory work, but that could
        // break things if we release a new binary that writes the wrong thing (e.g. like the issue
        // we historically had with writing to and reading from the wrong shard for filenodes). It
        // is therefore  generally preferable if the release schedule for things that might write
        // to our underlying storage is more controlled and has additional healthchecks, so not
        // doing it in Microwave at all is preferable.
        unimplemented!("MicrowaveFilenodes: unexpected add_filenodes")
    }

    async fn add_or_replace_filenodes(
        &self,
        _ctx: &CoreContext,
        _info: Vec<PreparedFilenode>,
    ) -> Result<FilenodeResult<()>> {
        // Same as above
        unimplemented!("MicrowaveFilenodes: unexpected add_or_replace_filenodes",)
    }

    async fn get_filenode(
        &self,
        ctx: &CoreContext,
        path: &RepoPath,
        filenode_id: HgFileNodeId,
    ) -> Result<FilenodeResult<Option<FilenodeInfo>>> {
        let info = self
            .inner
            .get_filenode(ctx, path, filenode_id)
            .await?
            .do_not_handle_disabled_filenodes()?;

        if let Some(ref info) = info {
            self.recorder
                .clone()
                .send(PreparedFilenode {
                    path: path.clone(),
                    info: info.clone(),
                })
                .await?;
        }

        Ok(FilenodeResult::Present(info))
    }

    async fn get_all_filenodes_maybe_stale(
        &self,
        _ctx: &CoreContext,
        _path: &RepoPath,
        _limit: Option<u64>,
    ) -> Result<FilenodeRangeResult<Vec<FilenodeInfo>>> {
        // The rationale is a bit different to that in add() here, since this is a read method. The
        // idea here is that we do not expect cache warmup to call get_all_filenodes_maybe_stale,
        // so we don't do anything about it in Microwave (i.e. don't cache it). If cache warmup
        // starts requiring this, then Microwave will fail, and we can fix it. If we didn't panic
        // (and e.g. silently ignored the errors), then we could end be in a situation where our
        // Microwave caches don't have the data we need, but we don't notice.
        unimplemented!("MicrowaveFilenodes: unexpected get_all_filenodes_maybe_stale")
    }

    fn prime_cache(&self, ctx: &CoreContext, filenodes: &[PreparedFilenode]) {
        self.inner.prime_cache(ctx, filenodes)
    }
}
