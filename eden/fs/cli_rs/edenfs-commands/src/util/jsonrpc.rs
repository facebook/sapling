/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct Request {
    jsonrpc: String,
    pub method: String,
    pub params: Option<serde_json::Value>,
    pub id: Option<String>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct Response {
    jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

impl Default for Response {
    fn default() -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: None,
            id: None,
        }
    }
}

impl From<anyhow::Error> for Response {
    fn from(error: anyhow::Error) -> Self {
        Self {
            error: Some(error.to_string().into()),
            ..Default::default()
        }
    }
}

#[derive(Debug, Default)]
pub struct ResponseBuilder {
    result: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
    id: Option<String>,
}

impl ResponseBuilder {
    pub fn result(result: serde_json::Value) -> Self {
        Self {
            result: Some(result),
            ..Default::default()
        }
    }

    pub fn error(error: &str) -> Self {
        Self {
            error: Some(error.into()),
            ..Default::default()
        }
    }

    pub fn with_id(mut self, id: Option<String>) -> Self {
        self.id = id;
        self
    }

    pub fn build(self) -> Response {
        Response {
            result: self.result,
            error: self.error,
            id: self.id,
            ..Default::default()
        }
    }
}
