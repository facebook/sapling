/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_limiter::AsyncLimiter;
use fbinit::FacebookInit;
use load_limiter::{BoxLoadLimiter, LoadCost, LoadLimiter, Metric};
use permission_checker::MononokeIdentitySet;
use scuba_ext::ScubaSampleBuilder;
use session_id::SessionId;
use slog::Logger;
use sshrelay::SshEnvVars;
use std::sync::Arc;
use std::time::Duration;
use tracing::TraceContext;

pub use self::builder::{generate_session_id, SessionContainerBuilder};
use crate::core::CoreContext;
use crate::is_quicksand;
use crate::logging::LoggingContainer;

mod builder;

#[derive(Clone)]
pub struct SessionContainer {
    fb: FacebookInit,
    inner: Arc<SessionContainerInner>,
}

struct SessionContainerInner {
    session_id: SessionId,
    trace: TraceContext,
    user_unix_name: Option<String>,
    source_hostname: Option<String>,
    ssh_env_vars: SshEnvVars,
    identities: Option<MononokeIdentitySet>,
    load_limiter: Option<BoxLoadLimiter>,
    blobstore_write_limiter: Option<AsyncLimiter>,
    blobstore_read_limiter: Option<AsyncLimiter>,
}

impl SessionContainer {
    pub fn builder(fb: FacebookInit) -> SessionContainerBuilder {
        SessionContainerBuilder::new(fb)
    }

    pub fn new_with_defaults(fb: FacebookInit) -> Self {
        Self::builder(fb).build()
    }

    pub fn new_context(&self, logger: Logger, scuba: ScubaSampleBuilder) -> CoreContext {
        let logging = LoggingContainer::new(logger, scuba);

        CoreContext::new_with_containers(self.fb, logging, self.clone())
    }

    pub fn fb(&self) -> FacebookInit {
        self.fb
    }

    pub fn session_id(&self) -> &SessionId {
        &self.inner.session_id
    }

    pub fn trace(&self) -> &TraceContext {
        &self.inner.trace
    }

    pub fn user_unix_name(&self) -> &Option<String> {
        &self.inner.user_unix_name
    }

    pub fn source_hostname(&self) -> &Option<String> {
        &self.inner.source_hostname
    }

    pub fn ssh_env_vars(&self) -> &SshEnvVars {
        &self.inner.ssh_env_vars
    }

    pub fn identities(&self) -> Option<&MononokeIdentitySet> {
        self.inner.identities.as_ref()
    }

    pub fn load_limiter(&self) -> Option<&dyn LoadLimiter> {
        match self.inner.load_limiter {
            Some(ref load_limiter) => Some(&**load_limiter),
            None => None,
        }
    }

    pub fn bump_load(&self, metric: Metric, load: LoadCost) {
        if let Some(limiter) = self.load_limiter() {
            limiter.bump_load(metric, load)
        }
    }

    pub async fn should_throttle(&self, metric: Metric, duration: Duration) -> Result<bool, !> {
        match &self.inner.load_limiter {
            Some(limiter) => match limiter.should_throttle(metric, duration).await {
                Ok(res) => Ok(res),
                Err(_) => Ok(false),
            },
            None => Ok(false),
        }
    }

    pub fn is_quicksand(&self) -> bool {
        is_quicksand(self.ssh_env_vars())
    }

    pub fn blobstore_read_limiter(&self) -> &Option<AsyncLimiter> {
        &self.inner.blobstore_read_limiter
    }

    pub fn blobstore_write_limiter(&self) -> &Option<AsyncLimiter> {
        &self.inner.blobstore_write_limiter
    }
}
