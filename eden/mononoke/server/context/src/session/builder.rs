/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_limiter::AsyncLimiter;
use fbinit::FacebookInit;
use load_limiter::BoxLoadLimiter;
use sshrelay::Metadata;
use std::sync::Arc;
use tracing::TraceContext;

use super::{SessionClass, SessionContainer, SessionContainerInner};

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
                trace: TraceContext::default(),
                metadata: Metadata::default(),
                load_limiter: None,
                blobstore_write_limiter: None,
                blobstore_read_limiter: None,
                session_class: SessionClass::UserWaiting,
            },
        }
    }

    pub fn trace(mut self, value: TraceContext) -> Self {
        self.inner.trace = value;
        self
    }

    pub fn metadata(mut self, value: Metadata) -> Self {
        self.inner.metadata = value;
        self
    }

    pub fn load_limiter(mut self, value: impl Into<Option<BoxLoadLimiter>>) -> Self {
        self.inner.load_limiter = value.into();
        self
    }

    pub fn blobstore_read_limiter(&mut self, limiter: AsyncLimiter) {
        self.inner.blobstore_read_limiter = Some(limiter);
    }

    pub fn blobstore_write_limiter(&mut self, limiter: AsyncLimiter) {
        self.inner.blobstore_write_limiter = Some(limiter);
    }

    pub fn session_class(mut self, value: SessionClass) -> Self {
        self.inner.session_class = value;
        self
    }
}
