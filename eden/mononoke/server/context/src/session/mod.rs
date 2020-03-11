/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use fbinit::FacebookInit;
use scuba_ext::ScubaSampleBuilder;
use session_id::SessionId;
use slog::Logger;
use sshrelay::SshEnvVars;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::TraceContext;

pub use self::builder::{generate_session_id, SessionContainerBuilder};
use crate::core::CoreContext;
#[cfg(fbcode_build)]
use crate::facebook::SessionFacebookData;
use crate::logging::LoggingContainer;

mod builder;

#[derive(Debug, Clone)]
pub struct SessionContainer {
    fb: FacebookInit,
    inner: Arc<SessionContainerInner>,
}

#[derive(Debug)]
struct SessionContainerInner {
    session_id: SessionId,
    trace: TraceContext,
    user_unix_name: Option<String>,
    source_hostname: Option<String>,
    ssh_env_vars: SshEnvVars,
    blobstore_semaphore: Option<Semaphore>,
    #[cfg(fbcode_build)]
    facebook_data: SessionFacebookData,
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

    pub fn blobstore_semaphore(&self) -> Option<&Semaphore> {
        self.inner.blobstore_semaphore.as_ref()
    }

    #[cfg(fbcode_build)]
    pub(crate) fn facebook_data(&self) -> &SessionFacebookData {
        &self.inner.facebook_data
    }
}
