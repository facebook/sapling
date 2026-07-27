/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::num::NonZeroU32;
use std::sync::Arc;

use async_limiter::AsyncLimiter;
use fbinit::FacebookInit;
use governor::Quota;
use governor::RateLimiter;
use metadata::Metadata;
use rate_limiting::BoxRateLimiter;

use super::SessionClass;
use super::SessionContainer;
use super::SessionContainerInner;

pub struct SessionContainerBuilder {
    fb: FacebookInit,
    metadata: Arc<Metadata>,
    inner: SessionContainerInner,
    session_class: SessionClass,
}

impl SessionContainerBuilder {
    pub fn build(self) -> SessionContainer {
        SessionContainer {
            fb: self.fb,
            metadata: self.metadata,
            inner: Arc::new(self.inner),
            session_class: self.session_class,
        }
    }

    pub fn new(fb: FacebookInit) -> Self {
        Self {
            fb,
            metadata: Arc::new(Metadata::default()),
            inner: SessionContainerInner {
                rate_limiter: None,
                blobstore_write_limiter: None,
                blobstore_read_limiter: None,
                readonly: false,
                server_side_tenting: false,
            },
            session_class: SessionClass::UserWaiting,
        }
    }

    pub fn metadata(mut self, value: Arc<Metadata>) -> Self {
        self.metadata = value;
        self
    }

    pub fn rate_limiter(mut self, value: impl Into<Option<BoxRateLimiter>>) -> Self {
        self.inner.rate_limiter = value.into();
        self
    }

    pub fn blobstore_read_limiter(mut self, limiter: AsyncLimiter) -> Self {
        self.inner.blobstore_read_limiter = Some(limiter);
        self
    }

    pub async fn blobstore_maybe_read_qps_limiter(mut self, qps: impl TryInto<u32>) -> Self {
        if let Ok(Some(qps)) = qps.try_into().map(NonZeroU32::new) {
            self.inner.blobstore_read_limiter =
                Some(AsyncLimiter::new(RateLimiter::direct(Quota::per_second(qps))).await);
        }
        self
    }

    pub fn blobstore_write_limiter(mut self, limiter: AsyncLimiter) -> Self {
        self.inner.blobstore_write_limiter = Some(limiter);
        self
    }

    pub async fn blobstore_maybe_write_qps_limiter(mut self, qps: impl TryInto<u32>) -> Self {
        if let Ok(Some(qps)) = qps.try_into().map(NonZeroU32::new) {
            self.inner.blobstore_write_limiter =
                Some(AsyncLimiter::new(RateLimiter::direct(Quota::per_second(qps))).await);
        }
        self
    }
    pub fn session_class(mut self, value: SessionClass) -> Self {
        self.session_class = value;
        self
    }

    pub fn readonly(mut self, readonly: bool) -> Self {
        self.inner.readonly = readonly;
        self
    }

    pub fn server_side_tenting(mut self, server_side_tenting: bool) -> Self {
        self.inner.server_side_tenting = server_side_tenting;
        self
    }
}
