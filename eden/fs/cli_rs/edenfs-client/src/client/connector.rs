/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use edenfs_error::Result;
use fbinit::FacebookInit;
use futures::future::BoxFuture;
use futures::future::Shared;

use crate::client::EdenFsThriftClient;
use crate::client::StreamingEdenFsThriftClient;

#[allow(dead_code)]
type EdenFsThriftClientFuture = Shared<BoxFuture<'static, Result<EdenFsThriftClient>>>;
#[allow(dead_code)]
type StreamingEdenFsThriftClientFuture =
    Shared<BoxFuture<'static, Result<StreamingEdenFsThriftClient>>>;

pub(crate) struct EdenFsConnector {
    #[allow(dead_code)]
    fb: FacebookInit,
    #[allow(dead_code)]
    socket_file: PathBuf,
}

impl EdenFsConnector {
    pub(crate) fn new(fb: FacebookInit, socket_file: PathBuf) -> Self {
        Self { fb, socket_file }
    }

    #[allow(dead_code)]
    pub(crate) fn connect() -> Result<EdenFsThriftClient> {
        unimplemented!()
    }

    #[allow(dead_code)]
    pub(crate) fn connect_streaming() -> Result<StreamingEdenFsThriftClient> {
        unimplemented!()
    }
}
