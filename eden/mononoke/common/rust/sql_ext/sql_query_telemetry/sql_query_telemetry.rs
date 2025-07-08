/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clientinfo::ClientRequestInfo;
use fbinit::FacebookInit;
use metadata::Metadata;

/// Provides data and objects needed to log SQL query telemetry, e.g.
/// client request info, scuba logger.
#[derive(Clone, Debug)]
pub struct SqlQueryTelemetry {
    /// fbinit to create a scuba logger
    fb: FacebookInit,

    /// Request metadata, e.g. client identities, request information,
    /// client correlator.
    metadata: Metadata,
}

impl SqlQueryTelemetry {
    pub fn new(fb: FacebookInit, metadata: Metadata) -> Self {
        Self { fb, metadata }
    }

    pub fn fb(&self) -> &FacebookInit {
        &self.fb
    }

    pub fn client_request_info(&self) -> Option<&ClientRequestInfo> {
        self.metadata.client_request_info()
    }

    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }
}
