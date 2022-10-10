/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(try_blocks)]

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::LoggingDestination;
use serde::Serialize;

#[async_trait]
pub trait Loggable: Serialize + Send + Sync {
    #[cfg(fbcode_build)]
    async fn log_to_logger(&self, ctx: &CoreContext) -> Result<()>;

    #[cfg(not(fbcode_build))]
    async fn log_to_logger(&self, _ctx: &CoreContext) -> Result<()> {
        Ok(())
    }

    fn log_to_scribe(&self, ctx: &CoreContext, scribe_category: &str) -> Result<()> {
        let json_data = serde_json::to_string(self)?;
        ctx.scribe().offer(scribe_category, &json_data)?;
        Ok(())
    }

    async fn log(&self, ctx: &CoreContext, logging_destination: &LoggingDestination) {
        let res = match logging_destination {
            LoggingDestination::Logger => self.log_to_logger(ctx).await,
            LoggingDestination::Scribe { scribe_category } => {
                self.log_to_scribe(ctx, scribe_category)
            }
        };
        if let Err(err) = res {
            ctx.scuba().clone().log_with_msg(
                "Failed to log",
                Some(format!(
                    "dest: {:?}, type: {}, error: {}",
                    logging_destination,
                    std::any::type_name::<Self>(),
                    err
                )),
            )
        }
    }
}
