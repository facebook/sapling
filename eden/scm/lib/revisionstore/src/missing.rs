/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::env;

use types::RepoPath;

pub struct MissingInjection {
    inner: Option<MissingInjectionInner>,
}

struct MissingInjectionInner {
    missing: Vec<String>,
}

impl MissingInjection {
    pub fn new_from_env(name: &str) -> Self {
        match env::var(name) {
            Ok(v) => {
                let mut missing = vec![];
                for part in v.split(',') {
                    missing.push(part.to_string());
                }
                Self {
                    inner: Some(MissingInjectionInner { missing }),
                }
            }
            Err(_) => Self { inner: None },
        }
    }

    pub fn is_missing(&self, path: &RepoPath) -> bool {
        match self.inner {
            None => false,
            Some(ref inner) => {
                for matcher in &inner.missing {
                    if path.as_str().starts_with(matcher.as_str()) {
                        return true;
                    }
                }
                false
            }
        }
    }
}
