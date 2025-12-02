/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use async_limiter::AsyncLimiter;
use clientinfo::ClientInfo;
use fbinit::FacebookInit;
use metadata::Metadata;
use permission_checker::MononokeIdentitySetExt;
use rate_limiting::BoxRateLimiter;
use rate_limiting::LoadCost;
use rate_limiting::LoadShedResult;
use rate_limiting::Metric;
use rate_limiting::RateLimitReason;
use rate_limiting::RateLimitResult;
use rate_limiting::RateLimiter;
use rate_limiting::Scope;
use scribe_ext::Scribe;
use scuba_ext::MononokeScubaSampleBuilder;

pub use self::builder::SessionContainerBuilder;
use crate::core::CoreContext;
use crate::logging::LoggingContainer;

mod builder;

#[derive(Clone)]
pub struct SessionContainer {
    fb: FacebookInit,
    inner: Arc<SessionContainerInner>,
    session_class: SessionClass,
}

/// Represents the reason this session is running
#[derive(Clone, Copy)]
pub enum SessionClass {
    /// There is someone waiting for this session to complete.
    UserWaiting,
    /// The session is doing background work (e.g. backfilling).
    /// Wherever reasonable, prefer to slow down and wait for work to complete
    /// fully rather than pushing work out to other tasks.
    Background,
    /// Same as Background, but if some work is taking too long to complete
    /// then fallback to normal (i.e. UserWaiting) behavior
    BackgroundUnlessTooSlow,
    /// This session is used by the warm bookmarks cache.
    WarmBookmarksCache,
}

struct SessionContainerInner {
    metadata: Arc<Metadata>,
    rate_limiter: Option<BoxRateLimiter>,
    blobstore_write_limiter: Option<AsyncLimiter>,
    blobstore_read_limiter: Option<AsyncLimiter>,
    // Whether this session is supposed to be readonly, this will cause the right
    // AuthContext to constructed.
    readonly: bool,
}

impl SessionContainer {
    pub fn builder(fb: FacebookInit) -> SessionContainerBuilder {
        SessionContainerBuilder::new(fb)
    }

    pub fn new_with_defaults(fb: FacebookInit) -> Self {
        Self::builder(fb).build()
    }

    pub fn new_with_client_info(fb: FacebookInit, client_info: ClientInfo) -> Self {
        let mut metadata = Metadata::default();
        metadata.add_client_info(client_info);
        Self::builder(fb).metadata(Arc::new(metadata)).build()
    }

    pub fn new_context(&self, scuba: MononokeScubaSampleBuilder) -> CoreContext {
        let logging = LoggingContainer::new(self.fb, scuba);
        CoreContext::from_parts(self.fb, logging, self.clone())
    }

    pub fn new_context_with_scribe(
        &self,
        scuba: MononokeScubaSampleBuilder,
        scribe: Scribe,
    ) -> CoreContext {
        let mut logging = LoggingContainer::new(self.fb, scuba);
        logging.with_scribe(scribe);

        CoreContext::from_parts(self.fb, logging, self.clone())
    }

    pub fn fb(&self) -> FacebookInit {
        self.fb
    }

    pub fn metadata(&self) -> &Metadata {
        &self.inner.metadata
    }

    pub fn rate_limiter(&self) -> Option<&(dyn RateLimiter + Send + Sync)> {
        match self.inner.rate_limiter {
            Some(ref rate_limiter) => Some(&**rate_limiter),
            None => None,
        }
    }

    pub fn bump_load(&self, metric: Metric, scope: Scope, load: LoadCost) {
        if let Some(limiter) = self.rate_limiter() {
            limiter.bump_load(metric, scope, load)
        }
    }

    pub fn check_load_shed(
        &self,
        scuba: &mut MononokeScubaSampleBuilder,
    ) -> Result<(), RateLimitReason> {
        match &self.inner.rate_limiter {
            Some(limiter) => {
                let main_client_id = self
                    .metadata()
                    .client_info()
                    .and_then(|client_info| client_info.request_info.clone())
                    .and_then(|request_info| request_info.main_id);
                let atlas = self.metadata().clientinfo_atlas();
                match limiter.check_load_shed(
                    self.metadata().identities(),
                    main_client_id.as_deref(),
                    scuba,
                    atlas,
                ) {
                    LoadShedResult::Fail(reason) => Err(reason),
                    LoadShedResult::Pass => Ok(()),
                }
            }
            None => Ok(()),
        }
    }

    pub async fn check_rate_limit(
        &self,
        metric: Metric,
        scuba: &mut MononokeScubaSampleBuilder,
    ) -> Result<(), RateLimitReason> {
        match &self.inner.rate_limiter {
            Some(limiter) => {
                let main_client_id = self
                    .metadata()
                    .client_info()
                    .and_then(|client_info| client_info.request_info.clone())
                    .and_then(|request_info| request_info.main_id);
                let atlas = self.metadata().clientinfo_atlas();
                match limiter
                    .check_rate_limit(
                        metric,
                        self.metadata().identities(),
                        main_client_id.as_deref(),
                        scuba,
                        atlas,
                    )
                    .await
                    .unwrap_or(RateLimitResult::Pass)
                {
                    RateLimitResult::Pass => Ok(()),
                    RateLimitResult::Fail(reason) => Err(reason),
                }
            }
            None => Ok(()),
        }
    }

    pub fn is_quicksand(&self) -> bool {
        self.metadata().identities().is_quicksand()
    }

    pub fn is_readonly(&self) -> bool {
        self.inner.readonly
    }

    pub fn blobstore_read_limiter(&self) -> Option<&AsyncLimiter> {
        self.inner.blobstore_read_limiter.as_ref()
    }

    pub fn blobstore_write_limiter(&self) -> Option<&AsyncLimiter> {
        self.inner.blobstore_write_limiter.as_ref()
    }

    pub fn session_class(&self) -> SessionClass {
        self.session_class
    }

    pub fn override_session_class(&mut self, session_class: SessionClass) {
        self.session_class = session_class;
    }
}
