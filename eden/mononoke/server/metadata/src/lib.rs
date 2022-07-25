/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(result_flattening)]

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use clientinfo::ClientInfo;
use permission_checker::MononokeIdentitySet;
use permission_checker::MononokeIdentitySetExt;
use session_id::generate_session_id;
use session_id::SessionId;
use std::net::IpAddr;
use std::time::Duration;
use tokio::time::timeout;
use trust_dns_resolver::TokioAsyncResolver;

#[derive(Clone, Debug, Default)]
pub struct Metadata {
    session_id: SessionId,
    identities: MononokeIdentitySet,
    /// If the identities were proxied, this is the true and original
    /// identities from the request.
    original_identities: Option<MononokeIdentitySet>,
    client_debug: bool,
    client_ip: Option<IpAddr>,
    client_hostname: Option<String>,
    revproxy_region: Option<String>,
    raw_encoded_cats: Option<String>,
    client_info: Option<ClientInfo>,
}

impl Metadata {
    pub async fn new(
        session_id: Option<&String>,
        identities: MononokeIdentitySet,
        client_debug: bool,
        client_ip: Option<IpAddr>,
    ) -> Self {
        let session_id: SessionId = match session_id {
            Some(id) => SessionId::from_string(id.to_owned()),
            None => generate_session_id(),
        };

        // Hostname of the client is for non-critical use only. We're doing best-effort lookup here:
        // 1) We're extracting it from identities (which requires no remote calls)
        let client_hostname = if let Some(client_hostname) = identities.hostname() {
            Some(client_hostname.to_string())
        }
        // 2) If it's not there we're trying to look it up via reverse dns with timeout of 1s.
        else if let Some(client_ip) = client_ip {
            timeout(Duration::from_secs(1), Metadata::reverse_lookup(client_ip))
                .await
                .map_err(Error::from)
                .flatten()
                .ok()
        } else {
            None
        };

        Self {
            session_id,
            identities,
            original_identities: None,
            client_debug,
            client_ip,
            client_hostname,
            revproxy_region: None,
            raw_encoded_cats: None,
            client_info: None,
        }
    }

    // Reverse lookups an IP to associated hostname. Trailing dots are stripped
    // to remain compatible with historical logging and common usage of reverse
    // hostnames in other logs (even though trailing dot is technically more correct)
    async fn reverse_lookup(client_ip: IpAddr) -> Result<String> {
        // This parses /etc/resolv.conf on each request. Given that this should be in
        // the page cache and the parsing of the text is very minimal, this shouldn't
        // impact performance much. In case this does lead to performance issues we
        // could start caching this, which for now would be preferred to avoid as this
        // might lead to unexpected behavior if the system configuration changes.
        let resolver = TokioAsyncResolver::tokio_from_system_conf()?;
        resolver
            .reverse_lookup(client_ip)
            .await?
            .iter()
            .next()
            .map(|name| name.to_string().trim_end_matches('.').to_string())
            .ok_or_else(|| anyhow!("failed to do reverse lookup"))
    }

    pub fn add_raw_encoded_cats(&mut self, raw_encoded_cats: String) -> &mut Self {
        self.raw_encoded_cats = Some(raw_encoded_cats);
        self
    }

    pub fn add_revproxy_region(&mut self, revproxy_region: String) -> &mut Self {
        self.revproxy_region = Some(revproxy_region);
        self
    }

    pub fn add_client_info(&mut self, client_info: ClientInfo) -> &mut Self {
        self.client_info = Some(client_info);
        self
    }

    pub fn add_original_identities(&mut self, identities: MononokeIdentitySet) -> &mut Self {
        self.original_identities = Some(identities);
        self
    }

    pub fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub fn identities(&self) -> &MononokeIdentitySet {
        &self.identities
    }

    pub fn original_identities(&self) -> Option<&MononokeIdentitySet> {
        self.original_identities.as_ref()
    }

    pub fn raw_encoded_cats(&self) -> &Option<String> {
        &self.raw_encoded_cats
    }

    pub fn set_identities(mut self, identities: MononokeIdentitySet) -> Self {
        self.identities = identities;
        self
    }

    pub fn revproxy_region(&self) -> &Option<String> {
        &self.revproxy_region
    }

    pub fn client_debug(&self) -> bool {
        self.client_debug
    }

    pub fn client_ip(&self) -> Option<&IpAddr> {
        self.client_ip.as_ref()
    }

    pub fn client_hostname(&self) -> Option<&str> {
        self.client_hostname.as_deref()
    }

    pub fn set_client_hostname(mut self, client_hostname: Option<String>) -> Self {
        self.client_hostname = client_hostname;
        self
    }

    pub fn unix_name(&self) -> Option<&str> {
        for identity in self.identities() {
            if identity.id_type() == "USER" {
                return Some(identity.id_data());
            }
        }

        None
    }

    pub fn sandcastle_alias(&self) -> Option<&str> {
        self.client_info
            .as_ref()
            .and_then(|ci| ci.fb.sandcastle_alias())
    }

    pub fn clientinfo_u64tag(&self) -> Option<u64> {
        self.client_info.as_ref()?.u64token
    }

    pub fn sandcastle_nonce(&self) -> Option<&str> {
        self.client_info
            .as_ref()
            .and_then(|ci| ci.fb.sandcastle_nonce())
    }

    pub fn clientinfo_tw_job(&self) -> Option<&str> {
        self.client_info.as_ref().and_then(|ci| ci.fb.tw_job())
    }

    pub fn clientinfo_tw_task(&self) -> Option<&str> {
        self.client_info.as_ref().and_then(|ci| ci.fb.tw_task())
    }
}
