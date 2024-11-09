/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use configloader::config::ConfigSet;
use configloader::hg::ConfigSetHgExt;
use configloader::hg::RepoInfo;
use configloader::Text;
use minibench::bench;
use minibench::elapsed;
use repo_minimal_info::RepoMinimalInfo;

fn main() {
    bench("parse 645KB file", || {
        let mut config_file = String::new();
        for _ in 0..100 {
            for section in b'a'..=b'z' {
                config_file += &format!("[{ch}{ch}{ch}{ch}]\n", ch = section as char);
                for name in b'a'..=b'z' {
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
            cfg.load(RepoInfo::NoRepo, Default::default()).unwrap();
        })
    });

    bench("load repo", || {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        std::fs::create_dir(path.join(".hg")).unwrap();
        std::fs::create_dir(path.join(".sl")).unwrap();
        let repo = RepoMinimalInfo::from_repo_root(path.to_path_buf()).unwrap();
        elapsed(|| {
            let mut cfg = ConfigSet::new();
            cfg.load(RepoInfo::Disk(&repo), Default::default()).unwrap();
        })
    });
}
