/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::errors::IOContext;
use crate::errors::IOResult;

pub fn hostname() -> IOResult<String> {
    Ok(hostname::get()
        .io_context("error getting hostname")?
        .to_string_lossy()
        .into())
}
