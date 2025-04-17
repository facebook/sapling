/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use gotham::state::State;
use gotham_ext::error::HttpError;
use gotham_ext::util::read_header_value;
use gotham_ext::util::read_header_value_ignore_err;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GitHost {
    Corp,
    CorpX2pagent,
    X2P,
    Prod,
    Unknown(String),
}

impl GitHost {
    pub fn from_state_location_host(state: &mut State) -> Result<Self, HttpError> {
        let host_header: String = read_header_value(state, "Host")
            .ok_or_else(|| HttpError::e400(anyhow!("No 'Host' header")))?
            .map_err(|e| HttpError::e400(anyhow!("Header 'Host' not a valid utf-8 string: {e}")))?;
        let corpx2pagent: bool =
            read_header_value_ignore_err::<&str, String>(state, "corpx2pagent").is_some();
        match (host_header.as_str(), corpx2pagent) {
            ("git.c2p.facebook.net", true) => Ok(Self::CorpX2pagent),
            ("git.c2p.facebook.net", _) => Ok(Self::Corp),
            ("git.edge.x2p.facebook.net", _) => Ok(Self::X2P),
            ("git.internal.tfbnw.net", _) => Ok(Self::Prod),
            _ => Ok(Self::Unknown(host_header)),
        }
    }

    pub fn from_state_mononoke_host(state: &mut State) -> Result<Self, HttpError> {
        let host_header: String = read_header_value(state, "Host")
            .ok_or_else(|| HttpError::e400(anyhow!("No 'Host' header")))?
            .map_err(|e| HttpError::e400(anyhow!("Header 'Host' not a valid utf-8 string: {e}")))?;
        let corpx2pagent: bool =
            read_header_value_ignore_err::<&str, String>(state, "corpx2pagent").is_some();
        match (host_header.as_str(), corpx2pagent) {
            ("mononoke-git.c2p.facebook.net", true) => Ok(Self::CorpX2pagent),
            ("mononoke-git.c2p.facebook.net", _) => Ok(Self::Corp),
            ("mononoke-git.edge.x2p.facebook.net", _) => Ok(Self::X2P),
            ("mononoke-git.internal.tfbnw.net", _) => Ok(Self::Prod),
            _ => Ok(Self::Unknown(host_header)),
        }
    }
}
