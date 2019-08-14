// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

use clap::{App, SubCommand};
use mercurial_revlog::{revlog::Revlog, RevlogChangeset};
use mercurial_types::HgNodeHash;
use std::{path::PathBuf, str::FromStr};

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    App::new("read revlog repo content")
        .version("0.0.0")
        .about("read revlogs")
        .args_from_usage(
            r#"
             <PATH>   'path to .hg folder with revlogs'
        "#,
        )
        .subcommand(
            SubCommand::with_name("changeset")
                .about("reads changesets")
                .args_from_usage(
                    r#"
                     <HASH>                 'sha1 hash to read'
                     -p, --parsed           'parse blob as RevlogChangeset'
                "#,
                ),
        )
}

fn main() {
    let matches = setup_app().get_matches();

    let base: PathBuf = matches.value_of("PATH").unwrap().into();
    let store = base.as_path().join("store");

    match matches.subcommand() {
        ("changeset", Some(sub_m)) => {
            let changelog =
                Revlog::from_idx_with_data(store.join("00changelog.i"), None as Option<String>)
                    .expect("Failed to load changelog");

            let hash = sub_m.value_of("HASH").unwrap();
            let hash = HgNodeHash::from_str(hash).expect("Incorrect Sha1 hash provided");

            let raw = changelog
                .get_idx_by_nodeid(hash)
                .and_then(|idx| changelog.get_rev(idx))
                .expect("Changeset not found");

            println!("RAW: {:?}", raw);

            let changeset = RevlogChangeset::new(raw.clone()).expect("Failed to deserialize CS");

            if sub_m.is_present("parsed") {
                println!("");
                println!("PAR: {:?}", changeset);
            }

            let encoded = changeset.get_node().expect("Failed to serialize CS");

            if raw != encoded {
                println!("");
                println!("POTENTIAL PROBLEM: {:?}", encoded);
            }
        }
        _ => {
            println!("{}", matches.usage());
            ::std::process::exit(1);
        }
    }
}
