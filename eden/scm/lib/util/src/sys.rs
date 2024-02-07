/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub fn hostname() -> String {
    whoami::devicename()
}

pub fn username() -> String {
    if std::env::var_os("TESTTMP").is_some() || cfg!(test) {
        "test".to_owned()
    } else {
        whoami::username()
    }
}
