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
/// If `origin_url` is provided, relative urls will be expanded based on it.
pub fn parse_gitmodules(data: &[u8], origin_url: Option<&str>) -> Vec<Submodule> {
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
            let url = if let Some(base_url) = origin_url {
                join_url(base_url, value)
            } else {
                value.to_owned()
            };
            current.url = url;
        }
    }

    if current.is_complete() {
        submodules.push(current);
    }

    submodules
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
