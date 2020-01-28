/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use context::{LoggingContainer, SessionContainer};
use fbinit::FacebookInit;
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
}

impl FastReplayDispatcher {
    pub fn new(
        fb: FacebookInit,
        logger: Logger,
        scuba: ScubaSampleBuilder,
        repo: MononokeRepo,
    ) -> Self {
        let noop_wireproto = WireprotoLogging::new(fb, repo.reponame().clone(), None, None);

        Self {
            fb,
            logger,
            scuba,
            repo,
            wireproto_logging: Arc::new(noop_wireproto),
        }
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
}
