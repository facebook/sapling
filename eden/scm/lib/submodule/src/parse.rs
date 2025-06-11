/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt;
use std::sync::OnceLock;

use configmodel::Config;
use configmodel::ConfigExt;
use serde::Serialize;

#[derive(Default, Serialize)]
pub struct Submodule {
    pub name: String,
    pub url: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
    pub active: bool,
}

impl Submodule {
    fn is_complete(&self) -> bool {
        !self.name.is_empty() && !self.url.is_empty() && !self.path.is_empty()
    }
}

impl fmt::Display for Submodule {
    // human-readable format similar to .gitmodules
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "[submodule \"{}\"]", self.name)?;
        writeln!(f, "\turl={}", self.url)?;
        writeln!(f, "\tpath={}", self.path)?;
        if let Some(ref r) = self.r#ref {
            writeln!(f, "\tref={}", r)?;
        }
        writeln!(f, "\tactive={}", self.active)
    }
}

/// config_value("foo = 123", "foo") = "123"
fn config_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    // NOTE: For simplicity, does not support escaping documented in `git-config` yet.
    line.strip_prefix(key)
        .and_then(|l| l.trim_start().strip_prefix('='))
        .map(|l| l.trim())
}

/// Parse the `.gitmodules` file.
///
/// If `origin_url` is provided, relative urls will be expanded based on it.
///
/// By default, all submodules are "active". If `config` is provided,
/// submodules named `x` would be inactive if `submodule.active-x` is false,
/// or if `submodule.active-x` is not set, and `submodule.active` is false.
pub fn parse_gitmodules(
    data: &[u8],
    origin_url: Option<&str>,
    config: Option<&(dyn Config + Send + Sync)>,
) -> Vec<Submodule> {
    struct State<'a> {
        submodules: Vec<Submodule>,
        current: Submodule,
        current_active: Option<bool>,
        config: Option<&'a (dyn Config + Send + Sync)>,
        default_active: OnceLock<bool>,
    }

    impl State<'_> {
        fn push(&mut self) {
            let mut taken = Submodule::default();
            std::mem::swap(&mut taken, &mut self.current);
            if taken.is_complete() {
                taken.active = match self.current_active {
                    Some(v) => v,
                    None => match self.config {
                        Some(config) => {
                            match config.get_opt("submodule", &format!("active-{}", &taken.name)) {
                                Ok(Some(v)) => v,
                                _ => self.default_active(),
                            }
                        }
                        None => true,
                    },
                };
                self.submodules.push(taken);
                self.current_active = None;
            }
        }

        fn default_active(&self) -> bool {
            *self.default_active.get_or_init(|| match self.config {
                Some(config) => match config.get_opt::<bool>("submodule", "active") {
                    Ok(Some(v)) => v,
                    _ => true,
                },
                None => true,
            })
        }
    }

    let mut state = State {
        submodules: Vec::with_capacity(data.iter().filter(|&&b| b == b'[').count()),
        current: Submodule::default(),
        current_active: None,
        config,
        default_active: OnceLock::new(),
    };

    for line in String::from_utf8_lossy(data).lines() {
        let line = line.trim();
        if let Some(value) = line
            .strip_prefix("[submodule \"")
            .and_then(|r| r.strip_suffix("\"]"))
        {
            if state.current.is_complete() {
                state.push();
            }
            state.current.name = value.to_owned();
        } else if let Some(value) = config_value(line, "ref") {
            state.current.r#ref = Some(value.to_owned());
        } else if let Some(value) = config_value(line, "path") {
            state.current.path = value.to_owned();
        } else if let Some(value) = config_value(line, "url") {
            let url = if let Some(base_url) = origin_url {
                join_url(base_url, value)
            } else {
                value.to_owned()
            };
            state.current.url = url;
        } else if let Some(value) = config_value(line, "active") {
            state.current_active = Some(value == "true");
        }
    }
    state.push();

    state.submodules
}

pub(crate) fn join_url(mut base_url: &str, mut maybe_relative_url: &str) -> String {
    if !maybe_relative_url.starts_with(".") {
        // Probably an absolute URL
        return maybe_relative_url.to_owned();
    }

    loop {
        if let Some(rest) = maybe_relative_url.strip_prefix("../") {
            match base_url.rsplit_once('/') {
                Some((parent_base_url, _)) => {
                    base_url = parent_base_url;
                    maybe_relative_url = rest;
                }
                None => break,
            }
        } else if let Some(rest) = maybe_relative_url.strip_prefix("./") {
            maybe_relative_url = rest;
        } else {
            break;
        }
    }

    format!("{}/{}", base_url, maybe_relative_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join_url() {
        // No change for absolute URL.
        assert_eq!(
            join_url("https://a.com/b", "https://b.com/c"),
            "https://b.com/c"
        );

        // Relative to the origin url.
        assert_eq!(join_url("https://a.com/b", "./c/d"), "https://a.com/b/c/d");

        // Parent of the original url.
        assert_eq!(
            join_url("https://a.com/b/c", "../../d/e"),
            "https://a.com/d/e"
        );
    }
}
