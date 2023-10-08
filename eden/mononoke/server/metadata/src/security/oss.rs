/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;

pub fn is_client_untrusted<GetHeaderFn>(_headers: GetHeaderFn) -> Result<bool>
where
    GetHeaderFn: Fn(&str) -> Result<Option<String>>,
{
    Ok(false)
}
