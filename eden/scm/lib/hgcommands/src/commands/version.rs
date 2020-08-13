/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::NoOpts;
use super::Result;
use super::IO;

pub fn run(_opts: NoOpts, io: &mut IO) -> Result<u8> {
    io.write(format!(
        r#"Mercurial Distributed SCM (version {})
(see https://mercurial-scm.org for more information)

Copyright (C) 2005-2017 Matt Mackall and others
This is free software; see the source for copying conditions. There is NO
warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
"#,
        ::version::VERSION
    ))?;
    Ok(0)
}

pub fn name() -> &'static str {
    "version|vers|versi|versio"
}

pub fn doc() -> &'static str {
    "output version and copyright information"
}
