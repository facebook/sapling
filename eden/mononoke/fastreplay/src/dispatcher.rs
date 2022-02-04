/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context, Error};
use blobstore::Blobstore;
use context::{LoggingContainer, SessionContainer};
use fbinit::FacebookInit;
use metaconfig_types::RepoClientKnobs;
use metadata::Metadata;
use repo_client::{MononokeRepo, RepoClient, WireprotoLogging};
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;
use std::sync::Arc;

pub struct FastReplayDispatcher {
    fb: FacebookInit,
    logger: Logger,
    repo: MononokeRepo,
    wireproto_logging: Arc<WireprotoLogging>,
    remote_args_blobstore: Option<Arc<dyn Blobstore>>,
    repo_client_knobs: RepoClientKnobs,
}

impl FastReplayDispatcher {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        repo: MononokeRepo,
        remote_args_blobstore: Option<Arc<dyn Blobstore>>,
        repo_client_knobs: RepoClientKnobs,
    ) -> Result<Self, Error> {
        let noop_wireproto = WireprotoLogging::new(fb, repo.reponame().clone(), None, None, None)
            .context("While instantiating noop_wireproto")?;

        Ok(Self {
            fb,
            logger,
            repo,
            wireproto_logging: Arc::new(noop_wireproto),
            remote_args_blobstore,
            repo_client_knobs,
        })
    }

    pub fn client(
        &self,
        scuba: MononokeScubaSampleBuilder,
        client_hostname: Option<String>,
    ) -> RepoClient {
        let metadata = Metadata::default().set_client_hostname(client_hostname);
        let metadata = Arc::new(metadata);

        let logging = LoggingContainer::new(self.fb, self.logger.clone(), scuba);
        let session = SessionContainer::builder(self.fb)
            .metadata(metadata)
            .build();

        RepoClient::new(
            self.repo.clone(),
            session,
            logging,
            false, // Don't preserve raw bundle 2 (we don't push)
            self.wireproto_logging.clone(),
            None, // Don't push redirect (we don't push)
            self.repo_client_knobs.clone(),
            None, // No backup repo source
        )
    }

    pub async fn load_remote_args(&self, key: String) -> Result<String, Error> {
        let session = SessionContainer::new_with_defaults(self.fb);
        let ctx = session.new_context(
            self.logger.clone(),
            MononokeScubaSampleBuilder::with_discard(),
        );

        let blobstore = self
            .remote_args_blobstore
            .as_ref()
            .ok_or_else(|| Error::msg("Cannot load remote_args without a remote_args_blobstore"))?;

        let e = Error::msg(format!("Key not found: {}", &key));
        let bytes = blobstore.get(&ctx, &key).await?.ok_or(e)?;
        Ok(String::from_utf8(bytes.into_raw_bytes().to_vec())?)
    }
}
