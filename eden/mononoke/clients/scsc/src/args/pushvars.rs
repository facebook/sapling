/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Arguments for pushvars.

use std::collections::BTreeMap;

use anyhow::Result;

#[derive(Clone)]
struct PushvarEntry(String, String);

impl std::str::FromStr for PushvarEntry {
    type Err = anyhow::Error;

    fn from_str(txt: &str) -> Result<Self> {
        let mut pushvar_parts = txt.splitn(2, '=');
        match (pushvar_parts.next(), pushvar_parts.next()) {
            (Some(name), Some(value)) => Ok(Self(name.to_string(), value.to_string())),
            _ => {
                anyhow::bail!(
                    "Pushvar specification must be of the form 'name=value', received '{}'",
                    txt
                )
            }
        }
    }
}

#[derive(clap::Args, Clone)]
/// Add arguments for specifying pushvars.
pub(crate) struct PushvarArgs {
    #[clap(long)]
    /// Pushvar (name=value) to send with write operation
    pushvar: Vec<PushvarEntry>,
}

impl PushvarArgs {
    /// Get specified pushvars.
    pub(crate) fn into_pushvars(self) -> Option<BTreeMap<String, Vec<u8>>> {
        if !self.pushvar.is_empty() {
            Some(
                self.pushvar
                    .into_iter()
                    .map(|e| (e.0, e.1.into_bytes()))
                    .collect(),
            )
        } else {
            None
        }
    }
}
