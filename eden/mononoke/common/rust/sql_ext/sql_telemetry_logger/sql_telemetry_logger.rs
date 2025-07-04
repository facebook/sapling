/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use clientinfo::ClientRequestInfo;

/// Provides data and objects needed to log SQL query telemetry, e.g.
/// client request info, scuba logger.
#[derive(Clone, Debug)]
pub struct SqlTelemetryLogger {
    /// Provides client request info so that client correlator can be attached
    /// to the query.
    client_request_info: Option<ClientRequestInfo>,
}

impl SqlTelemetryLogger {
    pub fn empty() -> Self {
        Self {
            client_request_info: None,
        }
    }

    pub fn new(client_request_info: Option<ClientRequestInfo>) -> Self {
        Self {
            client_request_info,
        }
    }

    pub fn client_request_info(&self) -> Option<&ClientRequestInfo> {
        self.client_request_info.as_ref()
    }
}

impl From<ClientRequestInfo> for SqlTelemetryLogger {
    fn from(client_request_info: ClientRequestInfo) -> Self {
        Self {
            client_request_info: Some(client_request_info),
        }
    }
}

impl From<&ClientRequestInfo> for SqlTelemetryLogger {
    fn from(client_request_info: &ClientRequestInfo) -> Self {
        SqlTelemetryLogger::from(client_request_info.clone())
    }
}
