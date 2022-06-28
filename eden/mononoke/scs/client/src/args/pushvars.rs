/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Arguments for pushvars.

use std::collections::BTreeMap;

use anyhow::anyhow;
use anyhow::Result;
use clap::App;
use clap::Arg;
use clap::ArgMatches;

const ARG_PUSHVAR: &str = "PUSHVAR";

/// Add arguments for specifying pushvars.
pub(crate) fn add_pushvar_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(ARG_PUSHVAR)
            .long("pushvar")
            .help("Pushvar (name=value) to send with write operation")
            .takes_value(true)
            .multiple(true),
    )
}

/// Get specified pushvars.
pub(crate) fn get_pushvars(matches: &ArgMatches) -> Result<Option<BTreeMap<String, Vec<u8>>>> {
    match matches.values_of(ARG_PUSHVAR) {
        None => Ok(None),
        Some(pushvar_specs) => {
            let mut pushvars = BTreeMap::new();
            for pushvar_spec in pushvar_specs {
                let mut pushvar_parts = pushvar_spec.splitn(2, '=');
                match (pushvar_parts.next(), pushvar_parts.next()) {
                    (Some(name), Some(value)) => {
                        pushvars.insert(name.to_string(), value.to_string().into_bytes());
                    }
                    _ => {
                        return Err(anyhow!(
                            "Pushvar specification must be of the form 'name=value', received '{}'",
                            pushvar_spec
                        ));
                    }
                }
            }
            Ok(Some(pushvars))
        }
    }
}
