/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use gotham::state::FromState;
use gotham::state::State;
use hex;
use hyper::Body;
use hyper::Response;
use std::fs::File;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;

use super::Middleware;

use crate::socket_data::TlsSessionData;

pub struct TlsSessionDataMiddleware {
    log_file: Option<Arc<Mutex<File>>>,
}

impl TlsSessionDataMiddleware {
    pub fn new<L: AsRef<Path>>(log_file: Option<L>) -> Result<Self, Error> {
        let log_file: Result<_, Error> = log_file
            .map(|log_file| {
                let log_file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log_file)?;
                Ok(Arc::new(Mutex::new(log_file)))
            })
            .transpose();

        Ok(Self {
            log_file: log_file?,
        })
    }

    fn log(&self, state: &mut State) -> Option<()> {
        let mut log_file = self.log_file.as_ref()?.lock().expect("Poisoned lock");
        let session_data = TlsSessionData::try_take_from(state)?;

        // See https://developer.mozilla.org/en-US/docs/Mozilla/Projects/NSS/Key_Log_Format for
        // formatting.
        let payload = format!(
            "CLIENT_RANDOM {} {}\n",
            hex::encode(session_data.client_random.as_ref()),
            hex::encode(session_data.master_key.as_ref())
        );

        log_file.write_all(payload.as_bytes()).ok()?;

        Some(())
    }
}

#[async_trait::async_trait]
impl Middleware for TlsSessionDataMiddleware {
    async fn inbound(&self, state: &mut State) -> Option<Response<Body>> {
        self.log(state);
        None
    }
}
