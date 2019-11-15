/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("[{0}] watchman command line transport request failed\n[{0} error] {1}")]
    CommandLineTransportError(&'static str, String),

    #[error("[{0}] watchman unix socket transport request failed\n[{0} error] {1}")]
    UnixSocketTransportError(&'static str, String),

    #[error("[{0}] watchman windows named pipe transport request failed\n[{0} error] {1}")]
    WindowsNamedPipeTransportError(&'static str, String),

    #[error("watchman bser protocol parsing error {0}")]
    WatchmanBserParsingError(String),

    #[error("error while decoding watchman pdu {0}")]
    WatchmanError(String),
}
