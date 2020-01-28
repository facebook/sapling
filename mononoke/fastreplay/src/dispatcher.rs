/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{Context, Error};
use blobstore::Blobstore;
use context::{LoggingContainer, SessionContainer};
use fbinit::FacebookInit;
use futures::compat::Future01CompatExt;
use repo_client::{MononokeRepo, RepoClient, WireprotoLogging};
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use std::sync::Arc;

pub struct FastReplayDispatcher {
    fb: FacebookInit,
    scuba: ScubaSampleBuilder,
    logger: Logger,
    repo: MononokeRepo,
    wireproto_logging: Arc<WireprotoLogging>,
    remote_args_blobstore: Option<Arc<dyn Blobstore>>,
}

impl FastReplayDispatcher {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        scuba: ScubaSampleBuilder,
        repo: MononokeRepo,
        remote_args_blobstore: Option<Arc<dyn Blobstore>>,
    ) -> Result<Self, Error> {
        let noop_wireproto = WireprotoLogging::new(fb, repo.reponame().clone(), None, None, None)
            .context("While instantiating noop_wireproto")?;

        Ok(Self {
            fb,
            logger,
            scuba,
            repo,
            wireproto_logging: Arc::new(noop_wireproto),
            remote_args_blobstore,
        })
    }

    pub fn client(&self) -> RepoClient {
        let logging = LoggingContainer::new(self.logger.clone(), self.scuba.clone());
        let session = SessionContainer::new_with_defaults(self.fb);

        RepoClient::new(
            self.repo.clone(),
            session,
            logging,
            0,     // Don't validate hashes (TODO: Make this configurable)
            false, // Don't preserve raw bundle 2 (we don't push)
            false, // Don't allow pushes (we don't push)
            true,  // Support bundle2_listkeys
            self.wireproto_logging.clone(),
            None, // Don't push redirect (we don't push)
            None, // Don't push redirect (we don't push)
        )
    }

    pub async fn load_remote_args(&self, key: String) -> Result<String, Error> {
        let session = SessionContainer::new_with_defaults(self.fb);
        let ctx = session.new_context(self.logger.clone(), self.scuba.clone());

        let blobstore = self
            .remote_args_blobstore
            .as_ref()
            .ok_or_else(|| Error::msg("Cannot load remote_args without a remote_args_blobstore"))?;

        let e = Error::msg(format!("Key not found: {}", &key));
        let bytes = blobstore.get(ctx, key).compat().await?.ok_or(e)?;
        Ok(String::from_utf8(bytes.into_bytes().to_vec())?)
    }
}
