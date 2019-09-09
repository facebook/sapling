// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::sync::Arc;

use async_trait::async_trait;
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use source_control::server::SourceControlService;

use super::super::actor::Mononoke;

#[derive(Clone)]
pub struct SourceControlServiceImpl {
    mononoke: Arc<Mononoke>,
    logger: Logger,
    scuba_builder: ScubaSampleBuilder,
}

impl SourceControlServiceImpl {
    pub fn new(mononoke: Arc<Mononoke>, logger: Logger, scuba_builder: ScubaSampleBuilder) -> Self {
        Self {
            mononoke,
            logger,
            scuba_builder,
        }
    }
}

#[async_trait]
impl SourceControlService for SourceControlServiceImpl {}
