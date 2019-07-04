// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use itertools::Itertools;
use std::collections::BTreeMap;

use cmdlib::args;

use failure_ext::{err_msg, Result};

fn main() -> Result<()> {
    let app = args::MononokeApp {
        safe_writes: false,
        hide_advanced_args: true,
        default_glog: false,
    };
    let matches = app
        .build("Lint Mononoke config files")
        .version("0.0.0")
        .about("Check Mononoke server configs for syntax and sanity.")
        .args_from_usage(
            r#"
            -q --quiet 'Only print errors'
            -v --verbose 'Dump content of configs'
            "#,
        )
        .get_matches();

    let quiet = matches.is_present("quiet");
    let verbose = matches.is_present("verbose");

    // Most of the work is done here - this validates that the files are present,
    // are correctly formed, and have the right fields (not too many, not too few).
    let configs = match args::read_configs(&matches) {
        Err(err) => {
            eprintln!("Error loading configs: {:#?}", err);
            return Err(err);
        }
        Ok(configs) => configs,
    };

    if verbose {
        println!("Configs:\n{:#?}", configs)
    }

    // Keep track of what repo ids we've seen
    let mut repoids = BTreeMap::<_, Vec<_>>::new();
    // Have we seen something suspect?
    let mut bad = false;

    for (name, config) in &configs.repos {
        let (isbad, locality) = match (
            config.storage_config.dbconfig.is_local(),
            config.storage_config.blobstore.is_local(),
        ) {
            (true, true) => (false, "local"),
            (false, false) => (false, "remote"),
            (true, false) => (true, "MIXED - local DB, remote blobstore"),
            (false, true) => (true, "MIXED - remote DB, local blobstore"),
        };

        bad |= isbad;

        repoids
            .entry(config.repoid)
            .and_modify(|names| names.push(name.as_str()))
            .or_insert(vec![name.as_str()]);

        if isbad || !quiet {
            println!(
                "Repo {}: {} - enabled: {:?} locality: {}",
                config.repoid, name, config.enabled, locality
            );
        }
    }

    for (id, names) in repoids {
        assert!(!names.is_empty());
        if names.len() > 1 {
            eprintln!(
                "ERROR: Repo Id {} used for repos: {}",
                id,
                names.into_iter().join(", ")
            );
            bad = true;
        }
    }

    if bad {
        Err(err_msg("Anomaly detected"))
    } else {
        Ok(())
    }
}
