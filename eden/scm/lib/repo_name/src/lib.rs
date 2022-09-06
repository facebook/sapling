/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::convert::AsRef;

use anyhow::Context;
use anyhow::Result;
use percent_encoding::percent_decode_str;
use percent_encoding::utf8_percent_encode;
use percent_encoding::AsciiSet;
use percent_encoding::NON_ALPHANUMERIC;

/// All non-alphanumeric characters (except hypens, underscores, and periods)
/// found in the repo's name will be percent-encoded before being used in URLs.
/// Characters allowed in a repo name (like `+` and `/`) since they are reserved
/// characters according to RFC 3986 section 2.2 Reserved Characters (January 2005)
const RESERVED_CHARS: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'_')
    .remove(b'-')
    .remove(b'.')
    .add(b'+')
    .add(b'/');

pub fn encode_repo_name(repo_name: impl AsRef<str>) -> String {
    utf8_percent_encode(repo_name.as_ref(), RESERVED_CHARS).to_string()
}

pub fn decode_repo_name(repo_name_encoded: impl AsRef<str>) -> Result<String> {
    Ok(percent_decode_str(repo_name_encoded.as_ref())
        .decode_utf8()
        .context("Repo name must be utf-8 percent encoded")?
        .to_owned()
        .to_string())
}
