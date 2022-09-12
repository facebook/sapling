/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use fbinit::FacebookInit;
use land_service_if::server::LandService;
use land_service_if::services::land_service::ValidateFooExn;
use land_service_if::types::*;
use slog::Logger;

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct LandServiceImpl {
    pub(crate) fb: FacebookInit,
    pub(crate) logger: Logger,
}

pub(crate) struct LandServiceThriftImpl(LandServiceImpl);

impl LandServiceImpl {
    pub fn new(fb: FacebookInit, logger: Logger) -> Self {
        Self { fb, logger }
    }

    pub(crate) fn thrift_server(&self) -> LandServiceThriftImpl {
        LandServiceThriftImpl(self.clone())
    }
}

#[async_trait]
impl LandService for LandServiceThriftImpl {
    async fn validateFoo(&self, input: FooRequest) -> Result<FooResult, ValidateFooExn> {
        Ok(FooResult {
            is_valid: input.bar.is_empty(),
            ..Default::default()
        })
    }
}
