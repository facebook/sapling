/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Error};
use blobstore::Blobstore;
use context::{LoggingContainer, SessionContainer};
use fbinit::FacebookInit;
use repo_client::{MononokeRepo, RepoClient, WireprotoLogging};
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use std::sync::Arc;

pub struct FastReplayDispatcher {
    fb: FacebookInit,
    logger: Logger,
    repo: MononokeRepo,
    wireproto_logging: Arc<WireprotoLogging>,
    remote_args_blobstore: Option<Arc<dyn Blobstore>>,
    hash_validation_percentage: usize,
}

impl FastReplayDispatcher {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        repo: MononokeRepo,
        remote_args_blobstore: Option<Arc<dyn Blobstore>>,
        hash_validation_percentage: usize,
    ) -> Result<Self, Error> {
        let noop_wireproto = WireprotoLogging::new(fb, repo.reponame().clone(), None, None, None)
            .context("While instantiating noop_wireproto")?;

        Ok(Self {
            fb,
            logger,
            repo,
            wireproto_logging: Arc::new(noop_wireproto),
            remote_args_blobstore,
            hash_validation_percentage,
        })
    }

    pub fn client(&self, scuba: ScubaSampleBuilder, source_hostname: Option<String>) -> RepoClient {
        let logging = LoggingContainer::new(self.fb, self.logger.clone(), scuba);
        let session = SessionContainer::builder(self.fb)
            .source_hostname(source_hostname)
            .build();

        RepoClient::new(
            self.repo.clone(),
            session,
            logging,
            self.hash_validation_percentage,
            false, // Don't preserve raw bundle 2 (we don't push)
            self.wireproto_logging.clone(),
            None, // Don't push redirect (we don't push)
            None, // No need to query live commit sync config
        )
    }

    pub async fn load_remote_args(&self, key: String) -> Result<String, Error> {
        let session = SessionContainer::new_with_defaults(self.fb);
        let ctx = session.new_context(self.logger.clone(), ScubaSampleBuilder::with_discard());

        let blobstore = self
            .remote_args_blobstore
            .as_ref()
            .ok_or_else(|| Error::msg("Cannot load remote_args without a remote_args_blobstore"))?;

        let e = Error::msg(format!("Key not found: {}", &key));
        let bytes = blobstore.get(ctx, key).await?.ok_or(e)?;
        Ok(String::from_utf8(bytes.into_raw_bytes().to_vec())?)
    }
}
