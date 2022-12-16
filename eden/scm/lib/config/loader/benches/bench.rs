/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use configloader::config::ConfigSet;
use configloader::hg::ConfigSetHgExt;
use configloader::Text;
use minibench::bench;
use minibench::elapsed;

fn main() {
    bench("parse 645KB file", || {
        let mut config_file = String::new();
        for _ in 0..100 {
            for section in b'a'..b'z' {
                config_file += &format!("[{ch}{ch}{ch}{ch}]\n", ch = section as char);
                for name in b'a'..b'z' {
                    config_file += &format!("{ch}{ch}{ch} = {ch}{ch}{ch}\n", ch = name as char);
                }
            }
        }
        let text = Text::from(config_file);
        elapsed(|| {
            let mut cfg = ConfigSet::new();
            cfg.parse(text.clone(), &"bench".into());
        })
    });

    bench("load system and user", || {
        elapsed(|| {
            let mut cfg = ConfigSet::new();
            cfg.load::<String, String>(None, None).unwrap();
        })
    });

    bench("load repo", || {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        std::fs::create_dir(path.join(".hg")).unwrap();
        std::fs::create_dir(path.join(".sl")).unwrap();
        elapsed(|| {
            let mut cfg = ConfigSet::new();
            cfg.load::<String, String>(Some(path), None).unwrap();
        })
    });
}
