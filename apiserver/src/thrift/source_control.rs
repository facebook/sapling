/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::sync::Arc;

use async_trait::async_trait;
use fbinit::FacebookInit;
use mononoke_api::Mononoke;
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use source_control::server::SourceControlService;

#[derive(Clone)]
pub struct SourceControlServiceImpl {
    fb: FacebookInit,
    mononoke: Arc<Mononoke>,
    logger: Logger,
    scuba_builder: ScubaSampleBuilder,
}

impl SourceControlServiceImpl {
    pub fn new(
        fb: FacebookInit,
        mononoke: Arc<Mononoke>,
        logger: Logger,
        scuba_builder: ScubaSampleBuilder,
    ) -> Self {
        Self {
            fb,
            mononoke,
            logger,
            scuba_builder,
        }
    }
}

#[async_trait]
impl SourceControlService for SourceControlServiceImpl {}
