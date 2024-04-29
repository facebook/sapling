/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Result;
use configloader::config::ConfigSet;
use configloader::Config;

pub struct Submodule {
    pub name: String,
    pub url: String,
    pub path: String,
}

pub fn parse_submodules(data: &[u8]) -> Result<Vec<Submodule>> {
    // strip leading space so things don't look like multiline values
    let data = String::from_utf8_lossy(data)
        .lines()
        .filter_map(|l| match l.trim() {
            "" => None,
            l => Some(l),
        })
        .collect::<Vec<&str>>()
        .join("\n");

    let mut cfg = ConfigSet::new().named("gitmodules");
    let errors = cfg.parse(data, &".gitmodules".into());
    if !errors.is_empty() {
        bail!("error parsing .gitmodules: {:?}", errors);
    }

    let mut submodules = Vec::new();
    let prefix = "submodule \"";
    let suffix = "\"";
    for sec in cfg.sections().iter() {
        if sec.starts_with(prefix) && sec.ends_with(suffix) && sec.len() > prefix.len() {
            let (mut url, mut path) = (None, None);
            for key in cfg.keys(sec) {
                if key == "url" {
                    url = cfg.get_nonempty(sec, &key);
                } else if key == "path" {
                    path = cfg.get_nonempty(sec, &key);
                }
            }

            if let (Some(url), Some(path)) = (url, path) {
                submodules.push(Submodule {
                    name: sec[prefix.len()..sec.len() - suffix.len()].replace('.', "_"),
                    url: url.to_string(),
                    path: path.to_string(),
                })
            }
        }
    }
    Ok(submodules)
}
