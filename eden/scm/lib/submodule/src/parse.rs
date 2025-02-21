/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */
use serde::Serialize;

#[derive(Default, Serialize)]
pub struct Submodule {
    pub name: String,
    pub url: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#ref: Option<String>,
}

impl Submodule {
    fn is_complete(&self) -> bool {
        !self.name.is_empty() && !self.url.is_empty() && !self.path.is_empty()
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
pub fn parse_gitmodules(data: &[u8]) -> Vec<Submodule> {
    let mut submodules = Vec::with_capacity(data.iter().filter(|&&b| b == b'[').count());
    let mut current = Submodule::default();
    for line in String::from_utf8_lossy(data).lines() {
        let line = line.trim();
        if let Some(value) = line
            .strip_prefix("[submodule \"")
            .and_then(|r| r.strip_suffix("\"]"))
        {
            if current.is_complete() {
                submodules.push(current);
                current = Submodule::default();
            }
            current.name = value.to_owned();
        } else if let Some(value) = config_value(line, "ref") {
            current.r#ref = Some(value.to_owned());
        } else if let Some(value) = config_value(line, "path") {
            current.path = value.to_owned();
        } else if let Some(value) = config_value(line, "url") {
            current.url = value.to_owned();
        }
    }

    if current.is_complete() {
        submodules.push(current);
    }

    submodules
}
