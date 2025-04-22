/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;

use anyhow::Context;
use anyhow::Result;

pub fn hostname() -> String {
    if std::env::var_os("TESTTMP").is_some() || cfg!(test) {
        // Doesn't seem like we want to use the real hostname in tests.
        // Also, this may fix some debugruntest issues on mac due to whoami issues.
        "test-hostname".to_owned()
    } else {
        match whoami::fallible::hostname() {
            Ok(hostname) => hostname,
            Err(err) => {
                tracing::error!(?err, "error getting hostname");
                "<UNKNOWN HOSTNAME>".to_string()
            }
        }
    }
}

pub fn username() -> Result<String> {
    if std::env::var_os("TESTTMP").is_some() || cfg!(test) {
        Ok("test".to_owned())
    } else {
        std::env::var_os(if cfg!(windows) { "USERNAME" } else { "USER" })
            .context("to get username")
            .map(|k| k.to_string_lossy().to_string())
    }
}

pub fn shell_escape(args: &[impl AsRef<str>]) -> String {
    args.iter()
        .map(|a| shell_escape::escape(Cow::Borrowed(a.as_ref())))
        .collect::<Vec<_>>()
        .join(" ")
}
