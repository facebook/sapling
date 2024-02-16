/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use gotham::mime;
use gotham::state::State;
use gotham_ext::error::ErrorFormatter;
use mime::Mime;

pub struct GitErrorFormatter;

impl ErrorFormatter for GitErrorFormatter {
    type Body = Vec<u8>;

    fn format(&self, error: &Error, _state: &State) -> Result<(Self::Body, Mime), Error> {
        let message = format!("{:#}", error);
        Ok((message.as_bytes().to_vec(), mime::TEXT_PLAIN))
    }
}
