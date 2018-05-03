// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![deny(warnings)]

extern crate clap;
extern crate futures;
extern crate tokio_core;

extern crate blobrepo;
extern crate blobstore;
extern crate manifoldblob;

use clap::{App, Arg, SubCommand};
use futures::prelude::*;
use tokio_core::reactor::Core;

use blobrepo::RawNodeBlob;
use blobstore::Blobstore;
use manifoldblob::ManifoldBlob;

fn setup_app<'a, 'b>() -> App<'a, 'b> {
    App::new("revlog to blob importer")
        .version("0.0.0")
        .about("make blobs")
        .args_from_usage(
            "--manifold-bucket [BUCKET] 'manifold bucket (default: mononoke_prod)' \
             --manifold-prefix [PREFIX] 'manifold prefix (default empty)'",
        )
        .subcommand(
            SubCommand::with_name("fetch")
                .about("fetches blobs from manifold")
                .args_from_usage("[KEY]    'key of the blob to be fetched'")
                .arg(
                    Arg::with_name("decode_as")
                        .long("decode_as")
                        .short("d")
                        .takes_value(true)
                        .possible_values(&["raw_node_blob"])
                        .required(false)
                        .help("if provided decode the value"),
                ),
        )
}

fn main() {
    let matches = setup_app().get_matches();

    let bucket = matches
        .value_of("manifold-bucket")
        .unwrap_or("mononoke_prod");
    let prefix = matches.value_of("manifold-prefix").unwrap_or("");

    let mut core = Core::new().unwrap();
    let remote = core.remote();

    let blobstore = ManifoldBlob::new_with_prefix(bucket, prefix, vec![&remote]);

    let future = match matches.subcommand() {
        ("fetch", Some(sub_m)) => {
            let key = sub_m.value_of("KEY").unwrap();
            let decode_as = sub_m.value_of("decode_as");
            blobstore.get(key.to_string()).map(move |value| {
                println!("{:?}", value);
                if let Some(value) = value {
                    match decode_as {
                        Some("raw_node_blob") => {
                            println!("{:?}", RawNodeBlob::deserialize(&value.into()));
                        }
                        _ => (),
                    }
                }
            })
        }
        _ => {
            println!("{}", matches.usage());
            ::std::process::exit(1);
        }
    };

    match core.run(future) {
        Ok(()) => (),
        Err(err) => {
            println!("{:?}", err);
            ::std::process::exit(1);
        }
    }
}
