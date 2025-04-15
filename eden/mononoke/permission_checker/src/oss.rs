/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::bail;
#[cfg(fbcode_build)]
use async_trait::async_trait;
use itertools::Itertools;
use openssl::x509::X509;

use crate::identity::MononokeIdentity;
use crate::identity::MononokeIdentitySet;
use crate::identity::MononokeIdentitySetExt;

impl MononokeIdentity {
    pub fn reviewer_identities(_username: &str) -> MononokeIdentitySet {
        MononokeIdentitySet::new()
    }

    pub fn try_from_ssh_encoded(_encoded: &str) -> Result<MononokeIdentitySet> {
        bail!("Decoding from SSH Principals is not yet implemented for MononokeIdentity")
    }

    pub fn try_from_json_encoded(_: &str) -> Result<MononokeIdentitySet> {
        bail!("Decoding from JSON is not yet implemented for MononokeIdentity")
    }

    pub fn try_from_x509(cert: &X509) -> Result<MononokeIdentitySet> {
        let subject_vec: Result<Vec<_>> = cert
            .subject_name()
            .entries()
            .map(|entry| {
                Ok(format!(
                    "{}={}",
                    entry.object().nid().short_name()?,
                    entry.data().as_utf8()?
                ))
            })
            .collect();
        let subject_name = subject_vec?.as_slice().join(",");

        let mut idents = MononokeIdentitySet::new();
        idents.insert(MononokeIdentity::new("X509_SUBJECT_NAME", subject_name));
        Ok(idents)
    }
}

impl MononokeIdentitySetExt for MononokeIdentitySet {
    fn is_quicksand(&self) -> bool {
        false
    }

    fn is_proxygen_test_identity(&self) -> bool {
        false
    }

    fn hostprefix(&self) -> Option<&str> {
        None
    }

    fn hostname(&self) -> Option<&str> {
        None
    }

    fn username(&self) -> Option<&str> {
        None
    }

    fn main_client_identity(&self, _sandcastle_alias: Option<&str>) -> String {
        String::from("PLACEHOLDER_CLIENT_IDENTITY")
    }

    fn to_string(&self) -> String {
        self.iter().map(ToString::to_string).join(",")
    }

    fn identity_type_filtered_concat(&self, _id_type: &str) -> Option<String> {
        None
    }
}
