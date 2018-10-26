// Copyright 2018 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

extern crate bytes;
extern crate configparser;
extern crate minibench;

use bytes::Bytes;
use configparser::config::ConfigSet;
use minibench::{bench, elapsed};
use std::io::Write;

fn main() {
    bench("parse 645KB file", || {
        let mut config_file = Vec::new();
        for _ in 0..100 {
            for section in b'a'..b'z' {
                config_file
                    .write(format!("[{ch}{ch}{ch}{ch}]\n", ch = section as char).as_bytes())
                    .unwrap();
                for name in b'a'..b'z' {
                    config_file
                        .write(
                            format!("{ch}{ch}{ch} = {ch}{ch}{ch}\n", ch = name as char).as_bytes(),
                        )
                        .unwrap();
                }
            }
        }
        elapsed(|| {
            let mut cfg = ConfigSet::new();
            cfg.parse(Bytes::from(&config_file[..]), &"bench".into());
        })
    });
}
