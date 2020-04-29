/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use fbinit::FacebookInit;
use permission_checker::MononokeIdentitySet;
use rand::{self, distributions::Alphanumeric, thread_rng, Rng};
use session_id::SessionId;
use sshrelay::SshEnvVars;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tracing::TraceContext;

use super::{SessionContainer, SessionContainerInner};
#[cfg(fbcode_build)]
use crate::facebook::SessionFacebookData;

pub fn generate_session_id() -> SessionId {
    let s: String = thread_rng().sample_iter(&Alphanumeric).take(16).collect();
    SessionId::from_string(s)
}

pub struct SessionContainerBuilder {
    fb: FacebookInit,
    inner: SessionContainerInner,
}

impl SessionContainerBuilder {
    pub fn build(self) -> SessionContainer {
        SessionContainer {
            fb: self.fb,
            inner: Arc::new(self.inner),
        }
    }

    pub fn new(fb: FacebookInit) -> Self {
        Self {
            fb,
            inner: SessionContainerInner {
                session_id: generate_session_id(),
                trace: TraceContext::default(),
                user_unix_name: None,
                source_hostname: None,
                ssh_env_vars: SshEnvVars::default(),
                blobstore_semaphore: None,
                identities: None,
                #[cfg(fbcode_build)]
                facebook_data: SessionFacebookData::default(),
            },
        }
    }

    pub fn session_id(mut self, value: SessionId) -> Self {
        self.inner.session_id = value;
        self
    }

    pub fn trace(mut self, value: TraceContext) -> Self {
        self.inner.trace = value;
        self
    }

    pub fn user_unix_name(mut self, value: impl Into<Option<String>>) -> Self {
        self.inner.user_unix_name = value.into();
        self
    }

    pub fn source_hostname(mut self, value: impl Into<Option<String>>) -> Self {
        self.inner.source_hostname = value.into();
        self
    }

    pub fn ssh_env_vars(mut self, value: SshEnvVars) -> Self {
        self.inner.ssh_env_vars = value;
        self
    }

    pub fn blobstore_concurrency(mut self, concurrency: usize) -> Self {
        self.inner.blobstore_semaphore = Some(Semaphore::new(concurrency));
        self
    }

    pub fn identities(mut self, value: impl Into<Option<MononokeIdentitySet>>) -> Self {
        self.inner.identities = value.into();
        self
    }

    #[cfg(fbcode_build)]
    pub(crate) fn facebook_data(&mut self) -> &mut SessionFacebookData {
        &mut self.inner.facebook_data
    }
}
