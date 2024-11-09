/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use thiserror::Error;

#[derive(Debug, Error)]
#[error("unable to find formatter for template {0}")]
pub struct FormatterNotFound(pub String);

#[derive(Debug, Error)]
pub enum FormattingError {
    /// IO error likely caused by failure to write
    #[error("error writing command output: {0}")]
    WriterError(#[from] std::io::Error),

    /// Error caused when serializing using the JSON formatter
    #[error("Serializing Error")]
    JsonFormatterError(#[from] serde_json::Error),

    /// Non-IO error caused by `format_plain` application code
    #[error(transparent)]
    PlainFormattingError(#[from] anyhow::Error),
}
