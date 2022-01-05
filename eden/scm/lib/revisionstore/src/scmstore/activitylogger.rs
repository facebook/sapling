/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::io::Write;
use std::path::Path;
use std::time::Duration;
use std::time::SystemTime;

use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use types::Key;

use super::FileAttributes;

pub(crate) struct ActivityLogger {
    f: File,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ActivityLog {
    pub op: ActivityType,
    pub keys: Vec<Key>,
    pub attrs: FileAttributes,
    pub start_millis: u128,
    pub duration_millis: u128,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum ActivityType {
    FileFetch,
}

impl ActivityLogger {
    pub(crate) fn new(f: File) -> Self {
        ActivityLogger { f }
    }

    pub(crate) fn log_file_fetch(
        &mut self,
        keys: Vec<Key>,
        attrs: FileAttributes,
        dur: Duration,
    ) -> Result<()> {
        serde_json::to_writer(
            &mut self.f,
            &ActivityLog {
                op: ActivityType::FileFetch,
                keys,
                attrs,
                start_millis: (SystemTime::now() - dur)
                    .duration_since(SystemTime::UNIX_EPOCH)?
                    .as_millis(),
                duration_millis: dur.as_millis(),
            },
        )?;
        self.f.write_all(&[b'\n'])?;
        Ok(())
    }
}

pub fn log_iter<P: AsRef<Path>>(path: P) -> Result<impl Iterator<Item = Result<ActivityLog>>> {
    let file = File::open(path)?;
    Ok(BufReader::new(file).lines().map(|line| {
        let log: ActivityLog = serde_json::from_str(&line?)?;
        Ok(log)
    }))
}
