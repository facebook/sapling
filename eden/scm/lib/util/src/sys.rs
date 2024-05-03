/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
        std::env::var_os(if cfg!(windows) {
            "COMPUTERNAME"
        } else if cfg!(macos) {
            "HOST"
        } else {
            "HOSTNAME"
        })
        .map_or(None, |h| h.to_str().map(|s| s.to_string()))
        .unwrap_or("".to_owned())
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

pub fn shell_escape(args: &[String]) -> String {
    args.iter()
        .map(|a| shell_escape::escape(Cow::Borrowed(a.as_str())))
        .collect::<Vec<_>>()
        .join(" ")
}
