/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::Error;
use gotham::state::{FromState, State};
use hex;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use super::Middleware;

use crate::pre_state_data::TlsSessionData;

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

impl Middleware for TlsSessionDataMiddleware {
    fn inbound(&self, state: &mut State) {
        self.log(state);
    }
}
