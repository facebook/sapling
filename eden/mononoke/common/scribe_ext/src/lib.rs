/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg_attr(not(fbcode_build), allow(unused_crate_dependencies))]

use anyhow::anyhow;
use anyhow::Error;
use fbinit::FacebookInit;
#[cfg(fbcode_build)]
use scribe::ScribeClient;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use anyhow as _; // oss uses anyhow

#[cfg(not(fbcode_build))]
mod oss;

#[cfg(not(fbcode_build))]
pub use oss::ScribeClientImplementation;
#[cfg(fbcode_build)]
pub use scuba::ScribeClientImplementation;

#[derive(Clone)]
pub enum Scribe {
    Client(Arc<ScribeClientImplementation>),
    LogToFile(Arc<Mutex<PathBuf>>),
}

impl ::std::fmt::Debug for Scribe {
    fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
        match self {
            Self::Client(_) => f.debug_struct("Scribe::Client").finish(),
            Self::LogToFile(_) => f.debug_struct("Scribe::LogToFile").finish(),
        }
    }
}

impl Scribe {
    pub fn new(fb: FacebookInit) -> Self {
        Self::Client(Arc::new(ScribeClientImplementation::new(fb)))
    }

    pub fn new_to_file(dir_path: PathBuf) -> Self {
        Self::LogToFile(Arc::new(Mutex::new(dir_path)))
    }

    pub fn offer(&self, category: &str, sample: &str) -> Result<(), Error> {
        use Scribe::*;

        match self {
            Client(client) => {
                #[cfg(fbcode_build)]
                let res = client.offer(category, sample);

                #[cfg(not(fbcode_build))]
                let res = {
                    let _ = client;
                    Ok(())
                };

                res
            }
            LogToFile(dir_path) => {
                let dir_path = dir_path.lock().unwrap();
                let is_valid_category = category
                    .chars()
                    .all(|c| char::is_alphanumeric(c) || c == '-' || c == '_');
                if !is_valid_category {
                    return Err(anyhow!("invalid category: {}", category));
                }
                let filename = dir_path.join(category);
                let mut file = OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(filename)?;
                ::std::writeln!(file, "{}", sample)?;
                Ok(())
            }
        }
    }
}
