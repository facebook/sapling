/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::Write;

use bytes::Bytes;
use minibench::{bench, elapsed};

use configparser::config::ConfigSet;

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
