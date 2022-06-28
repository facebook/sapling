/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Error;
use gotham::state::State;
use gotham_ext::error::ErrorFormatter;
use gotham_ext::state_ext::StateExt;
use lfs_protocol::git_lfs_mime;
use lfs_protocol::ResponseError;
use mime::Mime;

pub struct LfsErrorFormatter;

impl ErrorFormatter for LfsErrorFormatter {
    type Body = Vec<u8>;

    fn format(&self, error: &Error, state: &State) -> Result<(Self::Body, Mime), Error> {
        let message = format!("{:#}", error);

        let res = ResponseError {
            message,
            documentation_url: None,
            request_id: Some(state.short_request_id().to_string()),
        };

        let body = serde_json::to_vec(&res).context("Failed to serialize error")?;

        Ok((body, git_lfs_mime()))
    }
}
