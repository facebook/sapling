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
        Ok(Self::classify_mononoke_host(
            host_header.as_str(),
            corpx2pagent,
        ))
    }

    /// Classify a client `Host` header into a [`GitHost`].
    ///
    /// Recognizes both the `mononoke-git.*` names and the `git.*` names that
    /// prod clients actually arrive with (via GLB/edge). Before the `git.*`
    /// arms were added, bundle-uri classified every real request as `Unknown`
    /// and failed with e.g. "Unknown Host header: git.internal.tfbnw.net",
    /// forcing full-pack fallback clones and burning the Git Clone SLI.
    fn classify_mononoke_host(host_header: &str, corpx2pagent: bool) -> Self {
        match (host_header, corpx2pagent) {
            ("mononoke-git.c2p.facebook.net", true) => Self::CorpX2pagent,
            ("mononoke-git.c2p.facebook.net", _) => Self::Corp,
            ("mononoke-git.edge.x2p.facebook.net", _) => Self::X2P,
            ("mononoke-git.internal.tfbnw.net", _) => Self::Prod,
            ("git.c2p.facebook.net", true) => Self::CorpX2pagent,
            ("git.c2p.facebook.net", _) => Self::Corp,
            ("git.edge.x2p.facebook.net", _) => Self::X2P,
            ("git.internal.tfbnw.net", _) => Self::Prod,
            _ => Self::Unknown(host_header.to_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GitHost;

    #[test]
    fn classify_mononoke_host_accepts_git_and_mononoke_git_hosts() {
        // The git.* hosts are what real prod clients arrive with; they must be
        // recognized or bundle-uri rejects every request ("Unknown Host header").
        assert_eq!(
            GitHost::classify_mononoke_host("git.internal.tfbnw.net", false),
            GitHost::Prod
        );
        assert_eq!(
            GitHost::classify_mononoke_host("git.c2p.facebook.net", false),
            GitHost::Corp
        );
        assert_eq!(
            GitHost::classify_mononoke_host("git.c2p.facebook.net", true),
            GitHost::CorpX2pagent
        );
        assert_eq!(
            GitHost::classify_mononoke_host("git.edge.x2p.facebook.net", false),
            GitHost::X2P
        );

        // The pre-existing mononoke-git.* names keep working.
        assert_eq!(
            GitHost::classify_mononoke_host("mononoke-git.internal.tfbnw.net", false),
            GitHost::Prod
        );
        assert_eq!(
            GitHost::classify_mononoke_host("mononoke-git.c2p.facebook.net", true),
            GitHost::CorpX2pagent
        );

        // Genuinely unknown hosts still map to Unknown.
        assert_eq!(
            GitHost::classify_mononoke_host("evil.example.com", false),
            GitHost::Unknown("evil.example.com".to_owned())
        );
    }
}
