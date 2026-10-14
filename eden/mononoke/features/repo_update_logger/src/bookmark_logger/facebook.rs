/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::future::Future;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use async_trait::async_trait;
use context::CoreContext;
use futures_retry::retry;
use logger_ext::Loggable;
use mononoke_bookmark_rust_logger::MononokeBookmarkLogger;
use mononoke_macros::mononoke;
use whence_logged::WhenceScribeLogged;

use super::PlainBookmarkInfo;

const BOOKMARK_NOTIFICATION_MAX_ATTEMPTS: usize = 3;
const BOOKMARK_NOTIFICATION_RETRY_DELAY: Duration = Duration::from_millis(100);
const DETACHED_BOOKMARK_NOTIFICATION_DELIVERY_JUST_KNOB: &str =
    "scm/mononoke:enable_detached_bookmark_notification_delivery";

async fn retry_bookmark_notification<F, Fut>(mut publish: F) -> Result<()>
where
    F: FnMut() -> Fut + Send,
    Fut: Future<Output = Result<()>> + Send,
{
    retry(move |_| publish(), BOOKMARK_NOTIFICATION_RETRY_DELAY)
        .binary_exponential_backoff()
        .max_attempts(BOOKMARK_NOTIFICATION_MAX_ATTEMPTS)
        .await
        .context("bookmark notification retries exhausted")?;
    Ok(())
}

fn bookmark_notification_delivery_is_detached(repo_name: &str) -> bool {
    justknobs::eval(
        DETACHED_BOOKMARK_NOTIFICATION_DELIVERY_JUST_KNOB,
        None,
        Some(repo_name),
    )
}

fn new_bookmark_logger(
    ctx: &CoreContext,
    info: &PlainBookmarkInfo,
) -> Result<MononokeBookmarkLogger> {
    // Logger is configured for production-only use, so bookmark moves initiated from a devserver
    // must also write to the production category rather than its sandbox default.
    MononokeBookmarkLogger::override_whence_scribe_logged(ctx.fb, WhenceScribeLogged::PROD);
    let mut logger = MononokeBookmarkLogger::new(ctx.fb);
    logger
        .set_repo_name(info.repo_name.clone())
        .set_bookmark_name(info.bookmark_name.clone())
        .set_bookmark_kind(info.bookmark_kind.clone());
    if let Some(value) = &info.old_bookmark_value {
        logger.set_old_bookmark_value(value.clone());
    }
    if let Some(value) = &info.new_bookmark_value {
        logger.set_new_bookmark_value(value.clone());
    }
    logger
        .set_operation(info.operation.clone())
        .set_update_reason(info.update_reason.clone());
    if let Some(client_request_info) = ctx.client_request_info() {
        logger.set_client_correlator(client_request_info.correlator.clone());
        logger.set_client_entry_point(client_request_info.entry_point.to_string());
    }
    logger.attach_raw_scribe_write_cat()?;
    Ok(logger)
}

/// Runs one generated Logger Response API call outside Tokio worker threads. The API delegates to
/// synchronous C++ `logger_.logResponse()` and waits for per-sink write status before returning.
async fn run_bookmark_notification_attempt<F>(publish: F) -> Result<()>
where
    F: FnOnce() -> Result<()> + Send + 'static,
{
    mononoke::spawn_blocking(publish)
        .await
        .context("joining bookmark notification Logger task")?
}

/// Detached delivery is best-effort during process shutdown. Terminal async errors are recorded in
/// Scuba because the surrounding `Loggable::log` API intentionally consumes telemetry failures.
fn spawn_bookmark_notification_task<F>(ctx: CoreContext, repo_name: String, task: F)
where
    F: Future<Output = Result<()>> + Send + 'static,
{
    std::mem::drop(mononoke::spawn_task(async move {
        if let Err(error) = task.await {
            ctx.scuba()
                .clone()
                .add("repo_name", repo_name)
                .add("scribe_log_failure", "bookmark")
                .add("scribe_log_error", format!("{error:#}"))
                .log_with_msg("Failed to log bookmark notification", None);
        }
    }));
}

#[async_trait]
impl Loggable for PlainBookmarkInfo {
    async fn log_to_logger(&self, ctx: &CoreContext) -> Result<()> {
        if !bookmark_notification_delivery_is_detached(&self.repo_name) {
            let mut logger = new_bookmark_logger(ctx, self)?;
            logger.log_async()?;
            return Ok(());
        }

        let info = self.clone();
        let logger_ctx = ctx.clone();
        let repo_name = self.repo_name.clone();
        spawn_bookmark_notification_task(ctx.clone(), repo_name, async move {
            retry_bookmark_notification(|| {
                let info = info.clone();
                let logger_ctx = logger_ctx.clone();
                async move {
                    run_bookmark_notification_attempt(move || {
                        let mut logger = new_bookmark_logger(&logger_ctx, &info)?;
                        let response = logger.log_response_api()?;
                        ensure!(
                            response.is_scribe_success(),
                            "bookmark notification Scribe write failed: status={}, remote_code={}",
                            response.scribe_write_status,
                            response.scribe_remote_code
                        );
                        Ok(())
                    })
                    .await
                }
            })
            .await
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
