/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use clientinfo::ClientInfo;
use clientinfo::ClientRequestInfo;
use fbinit::FacebookInit;
use metadata::Metadata;
use scribe_ext::Scribe;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::Logger;
use sql_query_telemetry::SqlQueryTelemetry;

use crate::logging::LoggingContainer;
use crate::logging::SamplingKey;
use crate::perf_counters::PerfCounters;
use crate::perf_counters_stack::PerfCountersStack;
use crate::session::SessionClass;
use crate::session::SessionContainer;

#[derive(Clone)]
pub struct CoreContext {
    pub fb: FacebookInit,
    session: SessionContainer,
    logging: LoggingContainer,
}

impl CoreContext {
    pub fn from_parts(
        fb: FacebookInit,
        logging: LoggingContainer,
        session: SessionContainer,
    ) -> Self {
        Self {
            fb,
            logging,
            session,
        }
    }

    pub fn new(fb: FacebookInit) -> Self {
        let session = SessionContainer::new_with_defaults(fb);
        session.new_context(MononokeScubaSampleBuilder::with_discard())
    }

    pub fn new_with_client_info(fb: FacebookInit, client_info: ClientInfo) -> Self {
        let mut metadata = Metadata::default();
        metadata.add_client_info(client_info);
        let session = SessionContainer::builder(fb)
            .metadata(Arc::new(metadata))
            .build();
        session.new_context(MononokeScubaSampleBuilder::with_discard())
    }

    pub fn new_with_scuba(fb: FacebookInit, scuba: MononokeScubaSampleBuilder) -> Self {
        let session = SessionContainer::new_with_defaults(fb);
        session.new_context(scuba)
    }

    // Context for bulk processing like scrubbing or bulk backfilling
    pub fn new_for_bulk_processing(fb: FacebookInit) -> Self {
        let session = SessionContainer::builder(fb)
            .session_class(SessionClass::Background)
            .build();
        session.new_context(MononokeScubaSampleBuilder::with_discard())
    }

    pub fn test_mock(fb: FacebookInit) -> Self {
        let session = SessionContainer::new_with_defaults(fb);

        Self::test_mock_session(session)
    }

    pub fn test_mock_session(session: SessionContainer) -> Self {
        session.new_context(MononokeScubaSampleBuilder::with_discard())
    }

    /// Create a new CoreContext, with a reset LoggingContainer. This is useful to reset perf
    /// counters. The existing CoreContext is unaffected.
    pub fn clone_and_reset(&self) -> Self {
        self.session.new_context(self.scuba().clone())
    }

    pub fn clone_and_sample(&self, sampling_key: SamplingKey) -> Self {
        Self {
            fb: self.fb,
            session: self.session.clone(),
            logging: self.logging.clone_and_sample(sampling_key),
        }
    }

    pub fn with_mutated_scuba(
        &self,
        mutator: impl FnOnce(MononokeScubaSampleBuilder) -> MononokeScubaSampleBuilder,
    ) -> Self {
        Self {
            fb: self.fb,
            session: self.session.clone(),
            logging: self.logging.with_mutated_scuba(mutator),
        }
    }

    pub fn logger(&self) -> &Logger {
        self.logging.logger()
    }

    pub fn sampling_key(&self) -> Option<&SamplingKey> {
        self.logging.sampling_key()
    }

    pub fn scuba(&self) -> &MononokeScubaSampleBuilder {
        self.logging.scuba()
    }

    pub fn perf_counters(&self) -> &PerfCountersStack {
        self.logging.perf_counters()
    }

    pub fn metadata(&self) -> &Metadata {
        self.session.metadata()
    }

    pub fn client_request_info(&self) -> Option<&ClientRequestInfo> {
        self.metadata().client_request_info()
    }

    pub fn session(&self) -> &SessionContainer {
        &self.session
    }

    pub fn session_mut(&mut self) -> &mut SessionContainer {
        &mut self.session
    }

    pub fn scribe(&self) -> &Scribe {
        self.logging.scribe()
    }

    pub fn fork_perf_counters(&mut self) -> Arc<PerfCounters> {
        self.logging.fork_perf_counters()
    }

    pub fn sql_query_telemetry(&self) -> SqlQueryTelemetry {
        let fb = self.fb.clone();
        let metadata = self.metadata();
        SqlQueryTelemetry::new(fb, metadata.clone())
    }

    pub fn client_correlator(&self) -> Option<&str> {
        self.metadata()
            .client_info()
            .and_then(|ci| ci.request_info.as_ref().map(|cri| cri.correlator.as_str()))
    }
}
